//! Dedicated OpenKrisp Microphone and OpenKrisp Speaker routing.
//! Policy compiles on all hosts for deterministic tests;
//! production selection is gated to macOS in `current_router`.

use super::*;
use audio_protocol::{EndpointAvailability, VirtualEndpointId};

pub const MICROPHONE_ENDPOINT_ID: &str = "macos:openkrisp-microphone";
pub const MICROPHONE_DEVICE_ID: &str = "coreaudio:OpenKrispMicrophone_UID";
const MICROPHONE_MIRROR_ID: &str = "coreaudio:OpenKrispMicrophone_2_UID";
pub const SPEAKER_ENDPOINT_ID: &str = "macos:openkrisp-speaker";
pub const SPEAKER_DEVICE_ID: &str = "coreaudio:OpenKrispSpeaker_UID";
const SPEAKER_MIRROR_ID: &str = "coreaudio:OpenKrispSpeaker_2_UID";

#[derive(Clone, Copy)]
struct Endpoint {
    id: &'static str,
    device: &'static str,
    name: &'static str,
    channels: u16,
}

const MICROPHONE: Endpoint = Endpoint {
    id: MICROPHONE_ENDPOINT_ID,
    device: MICROPHONE_DEVICE_ID,
    name: "OpenKrisp Microphone",
    channels: 2,
};
const SPEAKER: Endpoint = Endpoint {
    id: SPEAKER_ENDPOINT_ID,
    device: SPEAKER_DEVICE_ID,
    name: "OpenKrisp Speaker",
    channels: 16,
};
pub struct MacOsRouter;

impl PlatformAudioRouter for MacOsRouter {
    fn capabilities(&self) -> RoutingCapabilities {
        RoutingCapabilities {
            virtual_microphone: true,
            system_loopback: true,
            virtual_speaker: true,
            requires_external_driver: true,
            ..PhysicalOnlyRouter.capabilities()
        }
    }

    fn discover_endpoints(&self, inventory: &DeviceInventory) -> Vec<PlatformEndpoint> {
        vec![discover(inventory, MICROPHONE, false)]
    }

    fn discover_loopback_endpoints(&self, inventory: &DeviceInventory) -> Vec<PlatformEndpoint> {
        vec![discover(inventory, SPEAKER, true)]
    }

    fn prepare_route(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<PreparedPlatformRoute, RoutingError> {
        if matches!(request.source, SourceEndpoint::SystemLoopback(_)) {
            let mut prepared = incoming::prepare(
                request,
                inventory,
                self.discover_loopback_endpoints(inventory),
            )?;
            if matches!(prepared.input_device_id.as_str(), SPEAKER_DEVICE_ID) {
                prepared.capture_stereo_channels = Some(16);
            }
            return Ok(prepared);
        }
        self.validate_no_feedback_loop(request, inventory)?;
        match &request.destination {
            DestinationEndpoint::PhysicalOutput(_) => {
                PhysicalOnlyRouter.prepare_route(request, inventory)
            }
            DestinationEndpoint::VirtualMicrophone(_) => {
                let SourceEndpoint::PhysicalInput(input) = &request.source else {
                    return Err(RoutingError::UnverifiedSource);
                };
                Ok(prepared(
                    request,
                    find_device(inventory, input, true)?,
                    virtual_output(request, inventory)?,
                ))
            }
            _ => Err(RoutingError::UnsupportedEndpoint),
        }
    }

    fn validate_no_feedback_loop(
        &self,
        request: &RouteConfig,
        inventory: &DeviceInventory,
    ) -> Result<(), RoutingError> {
        if matches!(request.source, SourceEndpoint::SystemLoopback(_)) {
            return incoming::prepare(
                request,
                inventory,
                self.discover_loopback_endpoints(inventory),
            )
            .map(|_| ());
        }
        match &request.destination {
            DestinationEndpoint::VirtualMicrophone(_) => {
                let output = virtual_output(request, inventory)?;
                let SourceEndpoint::PhysicalInput(input) = &request.source else {
                    return Err(
                        if matches!(&request.source, SourceEndpoint::VirtualRouteInput(id) if id.as_str() == MICROPHONE_ENDPOINT_ID)
                        {
                            RoutingError::FeedbackLoop
                        } else {
                            RoutingError::UnverifiedSource
                        },
                    );
                };
                let input = find_device(inventory, input, true)?;
                validate_ancestry(&ancestry(input), &ancestry(output))?;
                if input.kind != DeviceKind::Physical || is_virtual_path(input) {
                    return Err(RoutingError::UnverifiedSource);
                }
                Ok(())
            }
            DestinationEndpoint::PhysicalOutput(_) => {
                let (input, output) = physical_devices(request, inventory)?;
                if is_virtual_path(input) || is_virtual_path(output) {
                    validate_ancestry(&ancestry(input), &ancestry(output))?;
                    return Err(RoutingError::UnsupportedEndpoint);
                }
                PhysicalOnlyRouter.validate_no_feedback_loop(request, inventory)
            }
            _ => Err(RoutingError::UnsupportedEndpoint),
        }
    }
}

fn discover(inventory: &DeviceInventory, endpoint: Endpoint, incoming: bool) -> PlatformEndpoint {
    let Endpoint {
        id,
        device: device_id,
        name,
        channels,
    } = endpoint;
    let candidates: Vec<_> = inventory
        .devices
        .iter()
        .filter(|device| device.id.as_str() == device_id)
        .collect();
    let device = candidates.first().copied();
    let availability = if inventory.host != "coreaudio" {
        EndpointAvailability::Unsupported
    } else if candidates.is_empty() {
        EndpointAvailability::Missing
    } else if candidates.len() == 1 && device.is_some_and(|device| valid_device(device, channels)) {
        EndpointAvailability::Ready
    } else {
        EndpointAvailability::Invalid
    };
    let ready = device.filter(|_| availability == EndpointAvailability::Ready);
    PlatformEndpoint {
        id: VirtualEndpointId::new(id).unwrap_or_else(|_| unreachable!("static ID")),
        name: name.to_owned(),
        availability,
        can_create: false,
        playback_device_id: ready.map(|device| device.id.clone()),
        capture_device_id: ready.map(|device| device.id.clone()),
        ancestry: device.map(ancestry).unwrap_or_default(),
        instructions: instructions(endpoint, incoming),
        setup_url: "https://github.com/saeta-eth/OpenKrisp/blob/main/docs/INSTALLATION.md"
            .to_owned(),
    }
}

fn virtual_output<'a>(
    request: &RouteConfig,
    inventory: &'a DeviceInventory,
) -> Result<&'a AudioDeviceDescriptor, RoutingError> {
    if inventory.host != "coreaudio" {
        return Err(RoutingError::UnsupportedPlatform);
    }
    let DestinationEndpoint::VirtualMicrophone(id) = &request.destination else {
        return Err(RoutingError::UnsupportedEndpoint);
    };
    if id.as_str() != MICROPHONE_ENDPOINT_ID {
        return Err(RoutingError::UnsupportedEndpoint);
    }
    match discover(inventory, MICROPHONE, false).availability {
        EndpointAvailability::Missing => Err(RoutingError::MissingEndpoint),
        EndpointAvailability::Invalid => Err(RoutingError::InvalidEndpoint),
        EndpointAvailability::Unsupported => Err(RoutingError::UnsupportedPlatform),
        EndpointAvailability::Ready => inventory
            .devices
            .iter()
            .find(|device| device.id.as_str() == MICROPHONE_DEVICE_ID)
            .ok_or(RoutingError::MissingEndpoint),
    }
}

fn valid_device(device: &AudioDeviceDescriptor, channels: u16) -> bool {
    device.host == "coreaudio"
        && device.kind == DeviceKind::Virtual
        && device.supports_input
        && device.supports_output
        && device
            .default_input_format
            .as_ref()
            .is_some_and(|format| format.channels.get() == channels)
        && device
            .default_output_format
            .as_ref()
            .is_some_and(|format| format.channels.get() == channels)
}

fn is_virtual_path(device: &AudioDeviceDescriptor) -> bool {
    matches!(
        device.id.as_str(),
        MICROPHONE_DEVICE_ID | MICROPHONE_MIRROR_ID | SPEAKER_DEVICE_ID | SPEAKER_MIRROR_ID
    )
}

fn ancestry(device: &AudioDeviceDescriptor) -> EndpointAncestry {
    EndpointAncestry {
        recursive_paths: if is_virtual_path(device) {
            // The public and optional mirror UIDs share the same driver's input/output buffer.
            let id = match device.id.as_str() {
                SPEAKER_DEVICE_ID | SPEAKER_MIRROR_ID => SPEAKER_DEVICE_ID,
                _ => MICROPHONE_DEVICE_ID,
            };
            vec![DeviceId::new(id).unwrap_or_else(|_| unreachable!("static ID"))]
        } else {
            vec![device.id.clone()]
        },
        complete: is_virtual_path(device) || device.kind == DeviceKind::Physical,
    }
}

fn instructions(endpoint: Endpoint, incoming: bool) -> Vec<String> {
    let mut result = if incoming {
        vec![
            format!("In your call app, choose {} as the speaker output, using stereo channels 1–2.", endpoint.name),
            "Select physical headphones or speakers as Incoming output in OpenKrisp. For speakers, check that microphone echo cancellation is active.".to_owned(),
            "Music stays in bypass until you enable speech noise reduction. Do not use aggregate or multi-output devices.".to_owned(),
        ]
    } else {
        vec![
            "Select your physical microphone in OpenKrisp, choose Virtual microphone, and start.".to_owned(),
            format!("In your call or recording app, select {} as the microphone (channels 1–2).", endpoint.name),
            "Allow microphone access for OpenKrisp and the receiving app in System Settings → Privacy & Security → Microphone.".to_owned(),
        ]
    };
    result.insert(0, "The OpenKrisp installer includes OpenKrisp Microphone and OpenKrisp Speaker. If missing, rerun the installer, restart when prompted, then refresh devices.".to_owned());
    result.push("For both directions, choose OpenKrisp Microphone as your call microphone and OpenKrisp Speaker as your call speaker, then start both OpenKrisp routes.".to_owned());
    result
}
