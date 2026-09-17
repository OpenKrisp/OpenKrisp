//! Standard BlackHole 2ch microphone and separate BlackHole 16ch incoming bus.
//! Policy compiles on all hosts for deterministic tests;
//! production selection is gated to macOS in `current_router`.

use super::*;
use audio_protocol::{EndpointAvailability, VirtualEndpointId};

pub const BLACKHOLE_ENDPOINT_ID: &str = "macos:blackhole-2ch";
pub const BLACKHOLE_DEVICE_ID: &str = "coreaudio:BlackHole2ch_UID";
const BLACKHOLE_MIRROR_ID: &str = "coreaudio:BlackHole2ch_2_UID";
pub const BLACKHOLE_INCOMING_ENDPOINT_ID: &str = "macos:blackhole-16ch";
pub const BLACKHOLE_INCOMING_DEVICE_ID: &str = "coreaudio:BlackHole16ch_UID";
const BLACKHOLE_INCOMING_MIRROR_ID: &str = "coreaudio:BlackHole16ch_2_UID";

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
        vec![discover_blackhole(inventory, false)]
    }

    fn discover_loopback_endpoints(&self, inventory: &DeviceInventory) -> Vec<PlatformEndpoint> {
        let incoming = discover_blackhole(inventory, true);
        // Retain the original ID for saved incoming-only profiles. Never silently migrate it.
        let mut endpoint = discover_blackhole(inventory, false);
        endpoint.instructions = vec![
            "Choose BlackHole 2ch as your call or media application's speaker output, then select physical headphones or speakers in OpenKrisp. For speakers, check that microphone echo cancellation is active.".to_owned(),
            "Music stays in bypass until you enable speech noise reduction. Do not use aggregate or multi-output devices.".to_owned(),
            "For simultaneous microphone and incoming audio, install BlackHole 16ch and select it here and as your call application's speaker. Keep BlackHole 2ch as the call microphone.".to_owned(),
        ];
        vec![incoming, endpoint]
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
            if prepared.input_device_id.as_str() == BLACKHOLE_INCOMING_DEVICE_ID {
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
                        if matches!(&request.source, SourceEndpoint::VirtualRouteInput(id) if id.as_str() == BLACKHOLE_ENDPOINT_ID)
                        {
                            RoutingError::FeedbackLoop
                        } else {
                            RoutingError::UnverifiedSource
                        },
                    );
                };
                let input = find_device(inventory, input, true)?;
                validate_ancestry(&ancestry(input), &ancestry(output))?;
                if input.kind != DeviceKind::Physical || is_blackhole_path(input) {
                    return Err(RoutingError::UnverifiedSource);
                }
                Ok(())
            }
            DestinationEndpoint::PhysicalOutput(_) => {
                let (input, output) = physical_devices(request, inventory)?;
                if is_blackhole_path(input) || is_blackhole_path(output) {
                    validate_ancestry(&ancestry(input), &ancestry(output))?;
                    return Err(RoutingError::UnsupportedEndpoint);
                }
                PhysicalOnlyRouter.validate_no_feedback_loop(request, inventory)
            }
            _ => Err(RoutingError::UnsupportedEndpoint),
        }
    }
}

fn discover_blackhole(inventory: &DeviceInventory, incoming: bool) -> PlatformEndpoint {
    let (id, device_id, name, channels) = if incoming {
        (
            BLACKHOLE_INCOMING_ENDPOINT_ID,
            BLACKHOLE_INCOMING_DEVICE_ID,
            "BlackHole 16ch",
            16,
        )
    } else {
        (
            BLACKHOLE_ENDPOINT_ID,
            BLACKHOLE_DEVICE_ID,
            "BlackHole 2ch",
            2,
        )
    };
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
    } else if candidates.len() == 1
        && device.is_some_and(|device| valid_blackhole(device, channels))
    {
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
        instructions: if incoming {
            vec![
            "The combined OpenKrisp installer includes BlackHole 16ch for incoming audio. If missing, rerun the installer and restart when prompted; source builds can use the official installer or brew install blackhole-16ch.".to_owned(),
            "In your call app, choose BlackHole 16ch as the speaker output, using stereo channels 1–2. OpenKrisp captures only those two channels; channels 3–16 are ignored.".to_owned(),
            "Select physical headphones or speakers as Incoming output in OpenKrisp. Keep BlackHole 2ch as your call app's microphone and start both OpenKrisp routes. For speakers, check that echo cancellation is active.".to_owned(),
            "Do not use aggregate or multi-output devices. Music stays in bypass until you enable speech noise reduction.".to_owned(),
        ]
        } else {
            vec![
            "The combined OpenKrisp installer includes BlackHole 2ch and 16ch. Restart your Mac if prompted, then reopen OpenKrisp. If BlackHole 2ch is missing, rerun the installer; source builds can use the official installer or brew install blackhole-2ch.".to_owned(),
            "Select your physical microphone in OpenKrisp, choose Virtual microphone, and start.".to_owned(),
            "In your call or recording app, select BlackHole 2ch as the microphone (channels 1–2). For incoming processing, choose BlackHole 16ch as the call speaker and start incoming audio in OpenKrisp with a physical output.".to_owned(),
            "Allow microphone access for OpenKrisp and the receiving app in System Settings → Privacy & Security → Microphone.".to_owned(),
        ]
        },
        setup_url: "https://existential.audio/blackhole/".to_owned(),
    }
}

fn virtual_output<'a>(
    request: &RouteConfig,
    inventory: &'a DeviceInventory,
) -> Result<&'a AudioDeviceDescriptor, RoutingError> {
    if inventory.host != "coreaudio" {
        return Err(RoutingError::UnsupportedPlatform);
    }
    if !matches!(&request.destination, DestinationEndpoint::VirtualMicrophone(id) if id.as_str() == BLACKHOLE_ENDPOINT_ID)
    {
        return Err(RoutingError::UnsupportedEndpoint);
    }
    match discover_blackhole(inventory, false).availability {
        EndpointAvailability::Missing => Err(RoutingError::MissingEndpoint),
        EndpointAvailability::Invalid => Err(RoutingError::InvalidEndpoint),
        EndpointAvailability::Unsupported => Err(RoutingError::UnsupportedPlatform),
        EndpointAvailability::Ready => inventory
            .devices
            .iter()
            .find(|device| device.id.as_str() == BLACKHOLE_DEVICE_ID)
            .ok_or(RoutingError::MissingEndpoint),
    }
}

fn valid_blackhole(device: &AudioDeviceDescriptor, channels: u16) -> bool {
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

fn is_blackhole_path(device: &AudioDeviceDescriptor) -> bool {
    matches!(
        device.id.as_str(),
        BLACKHOLE_DEVICE_ID
            | BLACKHOLE_MIRROR_ID
            | BLACKHOLE_INCOMING_DEVICE_ID
            | BLACKHOLE_INCOMING_MIRROR_ID
    )
}

fn ancestry(device: &AudioDeviceDescriptor) -> EndpointAncestry {
    EndpointAncestry {
        recursive_paths: if is_blackhole_path(device) {
            // The public and optional mirror UIDs share the same driver's input/output buffer.
            let id = match device.id.as_str() {
                BLACKHOLE_INCOMING_DEVICE_ID | BLACKHOLE_INCOMING_MIRROR_ID => {
                    BLACKHOLE_INCOMING_DEVICE_ID
                }
                _ => BLACKHOLE_DEVICE_ID,
            };
            vec![DeviceId::new(id).unwrap_or_else(|_| unreachable!("static ID"))]
        } else {
            vec![device.id.clone()]
        },
        complete: is_blackhole_path(device) || device.kind == DeviceKind::Physical,
    }
}
