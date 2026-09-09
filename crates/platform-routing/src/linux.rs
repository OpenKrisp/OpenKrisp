//! PipeWire playback/capture endpoint discovery and route validation on Linux.

use super::*;
use audio_protocol::{EndpointAvailability, VirtualEndpointId};

pub const PIPEWIRE_ENDPOINT_ID: &str = "linux:pipewire";
pub const PIPEWIRE_PLAYBACK_ID: &str = "pipewire:openkrisp_sink";
pub const PIPEWIRE_CAPTURE_ID: &str = "pipewire:openkrisp_source";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinuxRouter;

impl LinuxRouter {
    #[must_use]
    pub const fn endpoint_id(self) -> &'static str {
        PIPEWIRE_ENDPOINT_ID
    }

    fn is_member(self, device: &AudioDeviceDescriptor) -> bool {
        matches!(
            device.id.as_str(),
            PIPEWIRE_PLAYBACK_ID | PIPEWIRE_CAPTURE_ID
        )
    }

    #[must_use]
    pub fn endpoint(self, inventory: &DeviceInventory) -> PlatformEndpoint {
        let members: Vec<_> = inventory
            .devices
            .iter()
            .filter(|device| self.is_member(device))
            .collect();
        let playback = members
            .iter()
            .copied()
            .find(|device| device.supports_output);
        let capture = members
            .iter()
            .copied()
            .find(|device| device.supports_input && !device.supports_output);
        let valid = members.len() == 2
            && playback.zip(capture).is_some_and(|(output, input)| {
                output.id != input.id
                    && members.iter().all(|device| {
                        device.host == "pipewire" && device.kind == DeviceKind::Virtual
                    })
                    && output
                        .default_output_format
                        .as_ref()
                        .is_some_and(|format| format.channels.get() == 2)
                    && input
                        .default_input_format
                        .as_ref()
                        .is_some_and(|format| format.channels.get() == 2)
                    && output.id.as_str() == PIPEWIRE_PLAYBACK_ID
                    && input.id.as_str() == PIPEWIRE_CAPTURE_ID
            });
        let availability = if inventory.host != "pipewire" {
            EndpointAvailability::Unsupported
        } else if members.is_empty() {
            EndpointAvailability::Missing
        } else if valid {
            EndpointAvailability::Ready
        } else {
            EndpointAvailability::Invalid
        };
        let ready = availability == EndpointAvailability::Ready;
        PlatformEndpoint {
            id: VirtualEndpointId::new(self.endpoint_id())
                .unwrap_or_else(|_| unreachable!("static ID")),
            name: "OpenKrisp Microphone".to_owned(),
            availability,
            can_create: inventory.host == "pipewire",
            playback_device_id: playback.filter(|_| ready).map(|device| device.id.clone()),
            capture_device_id: capture.filter(|_| ready).map(|device| device.id.clone()),
            ancestry: EndpointAncestry {
                recursive_paths: members.iter().map(|device| device.id.clone()).collect(),
                complete: ready,
            },
            instructions: vec![
                "Install OpenKrisp using your system package manager so it also installs PipeWire, pw-loopback, WirePlumber and PulseAudio compatibility. Log out and back in after first installation, then reopen OpenKrisp. PipeWire must be running in your desktop session.".to_owned(),
                "Choose your physical microphone and start Virtual microphone. OpenKrisp creates its PipeWire endpoints automatically and removes endpoints it owns when the route stops.".to_owned(),
                "After starting, select OpenKrisp Microphone in your call or recording app. For apps using PulseAudio, the pipewire-pulse compatibility service must also be running. Keep speakers set to headphones.".to_owned(),
            ],
            setup_url: "https://docs.pipewire.org/page_man_pw-loopback_1.html".to_owned(),
        }
    }

    /// Validate the source before any Linux graph creation, including on a missing endpoint.
    pub fn validate_source(
        self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<(), RoutingError> {
        if inventory.host != "pipewire" {
            return Err(RoutingError::UnsupportedPlatform);
        }
        if !matches!(&request.destination, DestinationEndpoint::VirtualMicrophone(id) if id.as_str() == self.endpoint_id())
        {
            return Err(RoutingError::UnsupportedEndpoint);
        }
        let SourceEndpoint::PhysicalInput(id) = &request.source else {
            return Err(
                if matches!(&request.source, SourceEndpoint::VirtualRouteInput(id) if id.as_str() == self.endpoint_id())
                {
                    RoutingError::FeedbackLoop
                } else {
                    RoutingError::UnverifiedSource
                },
            );
        };
        let source = find_device(inventory, id, true)?;
        if self.is_member(source) {
            return Err(RoutingError::FeedbackLoop);
        }
        if source.kind != DeviceKind::Physical || source.supports_output {
            return Err(RoutingError::UnverifiedSource);
        }
        Ok(())
    }
}

impl PlatformAudioRouter for LinuxRouter {
    fn capabilities(&self) -> RoutingCapabilities {
        RoutingCapabilities {
            virtual_microphone: true,
            ..PhysicalOnlyRouter.capabilities()
        }
    }
    fn discover_endpoints(&self, inventory: &DeviceInventory) -> Vec<PlatformEndpoint> {
        vec![self.endpoint(inventory)]
    }

    fn prepare_route(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<PreparedPlatformRoute, RoutingError> {
        self.validate_no_feedback_loop(request, inventory)?;
        if matches!(request.destination, DestinationEndpoint::PhysicalOutput(_)) {
            return PhysicalOnlyRouter.prepare_route(request, inventory);
        }
        let endpoint = self.endpoint(inventory);
        match endpoint.availability {
            EndpointAvailability::Missing => return Err(RoutingError::MissingEndpoint),
            EndpointAvailability::Invalid => return Err(RoutingError::InvalidEndpoint),
            EndpointAvailability::Unsupported => return Err(RoutingError::UnsupportedPlatform),
            EndpointAvailability::Ready => (),
        }
        let SourceEndpoint::PhysicalInput(input) = &request.source else {
            return Err(RoutingError::UnverifiedSource);
        };
        let output = endpoint
            .playback_device_id
            .ok_or(RoutingError::InvalidEndpoint)?;
        Ok(prepared(
            request,
            find_device(inventory, input, true)?,
            find_device(inventory, &output, false)?,
        ))
    }

    fn validate_no_feedback_loop(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<(), RoutingError> {
        match &request.destination {
            DestinationEndpoint::VirtualMicrophone(_) => self.validate_source(request, inventory),
            DestinationEndpoint::PhysicalOutput(_) => {
                let (input, output) = physical_devices(request, inventory)?;
                if self.is_member(input) && self.is_member(output) {
                    return Err(RoutingError::FeedbackLoop);
                }
                if self.is_member(input) || self.is_member(output) {
                    return Err(RoutingError::UnsupportedEndpoint);
                }
                PhysicalOnlyRouter.validate_no_feedback_loop(request, inventory)
            }
            _ => Err(RoutingError::UnsupportedEndpoint),
        }
    }
}
