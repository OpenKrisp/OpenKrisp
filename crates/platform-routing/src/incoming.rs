//! Dedicated application playback endpoints. Physical render output is never a capture monitor.
use super::*;
use audio_protocol::{EndpointAvailability, VirtualEndpointId};

pub const PIPEWIRE_INCOMING_ID: &str = "linux:pipewire-incoming";
pub const PIPEWIRE_INCOMING_PLAYBACK: &str = "pipewire:openkrisp_incoming_sink";
pub const PIPEWIRE_INCOMING_CAPTURE: &str = "pipewire:openkrisp_incoming_source";

pub fn linux_endpoint(inventory: &DeviceInventory) -> PlatformEndpoint {
    // Reuse the strictly checked sink/source pair contract with separate, fixed identities.
    let mut adapted = inventory.clone();
    adapted.devices.retain(|d| {
        !matches!(
            d.id.as_str(),
            linux::PIPEWIRE_PLAYBACK_ID | linux::PIPEWIRE_CAPTURE_ID
        )
    });
    for device in &mut adapted.devices {
        let id = match device.id.as_str() {
            PIPEWIRE_INCOMING_PLAYBACK => Some(linux::PIPEWIRE_PLAYBACK_ID),
            PIPEWIRE_INCOMING_CAPTURE => Some(linux::PIPEWIRE_CAPTURE_ID),
            _ => None,
        };
        if let Some(id) = id {
            device.id = DeviceId::new(id).unwrap_or_else(|_| unreachable!("static ID"));
        }
    }
    let mut endpoint = linux::LinuxRouter.endpoint(&adapted);
    endpoint.id =
        VirtualEndpointId::new(PIPEWIRE_INCOMING_ID).unwrap_or_else(|_| unreachable!("static ID"));
    endpoint.name = "OpenKrisp Incoming Audio".to_owned();
    let ready = endpoint.availability == EndpointAvailability::Ready;
    endpoint.playback_device_id = ready.then(|| {
        DeviceId::new(PIPEWIRE_INCOMING_PLAYBACK).unwrap_or_else(|_| unreachable!("static ID"))
    });
    endpoint.capture_device_id = ready.then(|| {
        DeviceId::new(PIPEWIRE_INCOMING_CAPTURE).unwrap_or_else(|_| unreachable!("static ID"))
    });
    endpoint.ancestry.recursive_paths = endpoint
        .playback_device_id
        .iter()
        .chain(endpoint.capture_device_id.iter())
        .cloned()
        .collect();
    endpoint.instructions = vec![
        "Start incoming audio, then choose OpenKrisp Incoming Audio as your call or media application's speaker output.".to_owned(),
        "Choose physical headphones in OpenKrisp. Music stays in bypass until you enable speech noise reduction.".to_owned(),
        "PipeWire, WirePlumber and pw-loopback must be available; endpoints created by this route disappear on Stop.".to_owned(),
    ];
    endpoint
}

/// Only a verified, explicitly selected physical render device is accepted for incoming audio.
pub fn physical_destination<'a>(
    request: &RouteConfig,
    inventory: &'a DeviceInventory,
) -> Result<&'a AudioDeviceDescriptor, RoutingError> {
    let DestinationEndpoint::PhysicalOutput(id) = &request.destination else {
        return Err(RoutingError::FeedbackLoop);
    };
    let device = find_device(inventory, id, false)?;
    if matches!(
        id.as_str(),
        "coreaudio:BlackHole2ch_UID"
            | "coreaudio:BlackHole2ch_2_UID"
            | linux::PIPEWIRE_PLAYBACK_ID
            | linux::PIPEWIRE_CAPTURE_ID
            | PIPEWIRE_INCOMING_PLAYBACK
            | PIPEWIRE_INCOMING_CAPTURE
    ) {
        return Err(RoutingError::FeedbackLoop);
    }
    if device.kind != DeviceKind::Physical {
        return Err(RoutingError::UnverifiedDestination);
    }
    Ok(device)
}

pub fn prepare(
    request: &RouteConfig,
    inventory: &DeviceInventory,
    endpoints: Vec<PlatformEndpoint>,
) -> Result<PreparedPlatformRoute, RoutingError> {
    let output = physical_destination(request, inventory)?;
    let SourceEndpoint::SystemLoopback(id) = &request.source else {
        return Err(RoutingError::UnsupportedEndpoint);
    };
    let endpoint = endpoints
        .into_iter()
        .find(|endpoint| endpoint.id.as_str() == id.as_str())
        .ok_or(RoutingError::UnsupportedEndpoint)?;
    match endpoint.availability {
        EndpointAvailability::Missing => return Err(RoutingError::MissingEndpoint),
        EndpointAvailability::Invalid => return Err(RoutingError::InvalidEndpoint),
        EndpointAvailability::Unsupported => return Err(RoutingError::UnsupportedPlatform),
        EndpointAvailability::Ready => (),
    }
    validate_ancestry(
        &endpoint.ancestry,
        &EndpointAncestry {
            recursive_paths: vec![output.id.clone()],
            complete: true,
        },
    )?;
    let capture = endpoint
        .capture_device_id
        .ok_or(RoutingError::InvalidEndpoint)?;
    Ok(prepared(
        request,
        find_device(inventory, &capture, true)?,
        output,
    ))
}

/// Reject shared virtual paths and competing render streams before either owner starts.
pub fn validate_concurrent(first: &RouteConfig, second: &RouteConfig) -> Result<(), RoutingError> {
    if first.destination == second.destination {
        return Err(RoutingError::EndpointInUse);
    }
    for (source, destination) in [
        (&first.source, &second.destination),
        (&second.source, &first.destination),
    ] {
        if let (
            SourceEndpoint::SystemLoopback(source),
            DestinationEndpoint::VirtualMicrophone(destination),
        ) = (source, destination)
            && source.as_str() == destination.as_str()
        {
            return Err(RoutingError::FeedbackLoop);
        }
    }
    Ok(())
}
