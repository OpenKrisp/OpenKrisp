#![deny(unsafe_code)]

//! Platform routing policy and owned Linux virtual endpoint lifecycle. No driver installation.

use audio_protocol::{
    AudioDeviceDescriptor, DestinationEndpoint, DeviceId, DeviceInventory, DeviceKind,
    EndpointAncestry, EngineErrorDto, ErrorCode, PlatformEndpoint, RouteConfig, RouteId,
    RoutingCapabilities, RoutingSnapshot, SourceEndpoint,
};

pub mod incoming;
pub mod linux;
#[cfg(feature = "macos-blackhole")]
pub mod macos;
pub mod pipewire;

// The only unsafe boundary is Linux's allocation-free, pre-exec parent-death setup.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod linux_process;

/// Prepared identifiers only. The route owner retains CPAL streams and any Linux endpoint
/// session separately. Existing drivers are never installed, removed or reconfigured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedPlatformRoute {
    pub route_id: RouteId,
    pub input_device_id: DeviceId,
    pub output_device_id: DeviceId,
    /// Capture channels 1–2 from an exact native channel count. Never applies to output.
    /// None retains ordinary mono/stereo negotiation. Recomputed on every recovery.
    pub capture_stereo_channels: Option<u16>,
}

pub trait PlatformAudioRouter {
    fn capabilities(&self) -> RoutingCapabilities;
    fn discover_endpoints(&self, inventory: &DeviceInventory) -> Vec<PlatformEndpoint>;
    fn discover_loopback_endpoints(&self, _inventory: &DeviceInventory) -> Vec<PlatformEndpoint> {
        Vec::new()
    }
    fn prepare_route(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<PreparedPlatformRoute, RoutingError>;
    /// Existing endpoint adapters have no OS graph to undo; stream teardown is the owner's job.
    fn release_route(&self, _route_id: &RouteId) -> Result<(), RoutingError> {
        Ok(())
    }
    fn validate_no_feedback_loop(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<(), RoutingError>;

    fn snapshot(&self, inventory: &DeviceInventory) -> RoutingSnapshot {
        RoutingSnapshot {
            capabilities: self.capabilities(),
            endpoints: self.discover_endpoints(inventory),
            loopback_endpoints: self.discover_loopback_endpoints(inventory),
        }
    }
}

/// Physical routes on platforms with no implemented virtual microphone integration.
pub struct PhysicalOnlyRouter;

impl PlatformAudioRouter for PhysicalOnlyRouter {
    fn capabilities(&self) -> RoutingCapabilities {
        RoutingCapabilities {
            physical_input: true,
            physical_output: true,
            ..Default::default()
        }
    }
    fn discover_endpoints(&self, _: &DeviceInventory) -> Vec<PlatformEndpoint> {
        Vec::new()
    }
    fn prepare_route(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<PreparedPlatformRoute, RoutingError> {
        let (input, output) = physical_devices(request, inventory)?;
        self.validate_no_feedback_loop(request, inventory)?;
        Ok(prepared(request, input, output))
    }
    fn validate_no_feedback_loop(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<(), RoutingError> {
        let (input, output) = physical_devices(request, inventory)?;
        if input.kind == DeviceKind::Virtual || output.kind == DeviceKind::Virtual {
            return Err(RoutingError::UnsupportedEndpoint);
        }
        // A physical duplex interface can safely be both source and destination.
        Ok(())
    }
}

#[must_use]
pub fn current_router() -> Box<dyn PlatformAudioRouter> {
    #[cfg(all(target_os = "macos", feature = "macos-blackhole"))]
    {
        Box::new(macos::MacOsRouter)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxRouter)
    }
    #[cfg(not(any(
        target_os = "linux",
        all(target_os = "macos", feature = "macos-blackhole")
    )))]
    {
        Box::new(PhysicalOnlyRouter)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RoutingError {
    #[error(
        "Incoming audio must play to verified physical headphones or speakers. Aggregate, virtual and unknown destinations cannot be excluded from capture."
    )]
    UnverifiedDestination,
    #[error("Another route already owns this output. Choose a separate output or stop that route.")]
    EndpointInUse,
    #[error("This virtual audio route is not supported on this platform or build.")]
    UnsupportedPlatform,
    #[error("The selected virtual endpoint is not supported. Choose an endpoint shown in setup.")]
    UnsupportedEndpoint,
    #[error(
        "The selected virtual audio endpoint is missing. Follow its setup instructions, then refresh devices."
    )]
    MissingEndpoint,
    #[error(
        "The selected virtual audio endpoint has an incomplete, ambiguous or unsupported device configuration."
    )]
    InvalidEndpoint,
    #[error(
        "The selected input or output device is unavailable. Refresh devices and select it again."
    )]
    DeviceUnavailable,
    #[error(
        "System-default audio aliases can change devices. Select a specific microphone and headphone output instead."
    )]
    DynamicDeviceAlias,
    #[error("This route would feed processed audio back into its input.")]
    FeedbackLoop,
    #[error(
        "The microphone must be a verified physical device. Aggregate, virtual and unclassified inputs are not supported for virtual microphone routing."
    )]
    UnverifiedSource,
    #[error(
        "The PipeWire audio endpoint could not start or stopped unexpectedly. Ensure PipeWire is running and pw-loopback is installed."
    )]
    EndpointCreationFailed,
}

impl RoutingError {
    #[must_use]
    pub fn to_dto(self) -> EngineErrorDto {
        let action = match self {
            Self::UnverifiedDestination | Self::EndpointInUse => {
                "Select a separate physical headphone output for incoming audio."
            }
            Self::MissingEndpoint | Self::InvalidEndpoint => {
                "Follow the selected endpoint's setup instructions. Check that both sides are enabled with the required channel count, then refresh devices."
            }
            Self::FeedbackLoop | Self::UnverifiedSource => {
                "Keep microphone and incoming audio on separate virtual paths. Select physical devices directly; avoid aggregate and multi-output devices."
            }
            Self::UnsupportedPlatform => {
                "Use BlackHole on macOS or an active PipeWire session on Linux."
            }
            Self::UnsupportedEndpoint => {
                "Choose a physical monitor output or the supported virtual microphone endpoint."
            }
            Self::DeviceUnavailable => {
                "Reconnect the selected device and refresh the device list. Routes never fall back to a different endpoint."
            }
            Self::DynamicDeviceAlias => {
                "Choose the device's own name rather than a PipeWire default alias, then start again."
            }
            Self::EndpointCreationFailed => {
                "Install your distribution's PipeWire tools (pipewire-bin on Debian/Ubuntu, pipewire-utils on Fedora, pipewire-audio on Arch), start your PipeWire session, and retry."
            }
        };
        EngineErrorDto::new(ErrorCode::Routing, self.to_string()).with_suggested_action(action)
    }
}

/// Reject known recursion first, then graphs whose ancestry cannot be proven complete.
pub fn validate_ancestry(
    source: &EndpointAncestry,
    destination: &EndpointAncestry,
) -> Result<(), RoutingError> {
    if source
        .recursive_paths
        .iter()
        .any(|path| destination.recursive_paths.contains(path))
    {
        return Err(RoutingError::FeedbackLoop);
    }
    if !source.complete || !destination.complete {
        return Err(RoutingError::UnverifiedSource);
    }
    Ok(())
}

fn physical_devices<'a>(
    request: &RouteConfig,
    inventory: &'a DeviceInventory,
) -> Result<(&'a AudioDeviceDescriptor, &'a AudioDeviceDescriptor), RoutingError> {
    let SourceEndpoint::PhysicalInput(input) = &request.source else {
        return Err(RoutingError::UnsupportedEndpoint);
    };
    let DestinationEndpoint::PhysicalOutput(output) = &request.destination else {
        return Err(RoutingError::UnsupportedPlatform);
    };
    Ok((
        find_device(inventory, input, true)?,
        find_device(inventory, output, false)?,
    ))
}

fn find_device<'a>(
    inventory: &'a DeviceInventory,
    id: &DeviceId,
    input: bool,
) -> Result<&'a AudioDeviceDescriptor, RoutingError> {
    // CPAL exposes these IDs as dynamic aliases, even though their kind is Unknown.
    // They must never become stored route identities or be followed during recovery.
    if matches!(
        id.as_str(),
        "pipewire:input_default" | "pipewire:output_default"
    ) {
        return Err(RoutingError::DynamicDeviceAlias);
    }
    let mut matches = inventory.devices.iter().filter(|device| &device.id == id);
    let device = matches.next().ok_or(RoutingError::DeviceUnavailable)?;
    if matches.next().is_some()
        || (input && !device.supports_input)
        || (!input && !device.supports_output)
    {
        return Err(RoutingError::DeviceUnavailable);
    }
    Ok(device)
}

fn prepared(
    request: &RouteConfig,
    input: &AudioDeviceDescriptor,
    output: &AudioDeviceDescriptor,
) -> PreparedPlatformRoute {
    PreparedPlatformRoute {
        route_id: request.id.clone(),
        input_device_id: input.id.clone(),
        output_device_id: output.id.clone(),
        capture_stereo_channels: None,
    }
}
