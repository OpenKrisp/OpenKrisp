use std::num::NonZeroU16;

use audio_protocol::{
    AudioDeviceDescriptor, AudioFormat, DenoiseSettings, DestinationEndpoint, DeviceId,
    DeviceInventory, DeviceKind, EndpointAncestry, ProcessingMode, RouteConfig, RouteId,
    SampleFormat, SampleRateHz, SourceEndpoint, VirtualEndpointId,
};
use platform_routing::{PhysicalOnlyRouter, PlatformAudioRouter, RoutingError, validate_ancestry};

fn device(id: &str, kind: DeviceKind) -> AudioDeviceDescriptor {
    let format = AudioFormat {
        sample_rate_hz: SampleRateHz::new(48_000).expect("rate"),
        channels: 2.try_into().expect("channels"),
        sample_format: SampleFormat::F32,
        buffer_size_frames: None,
    };
    AudioDeviceDescriptor {
        id: DeviceId::new(id).expect("ID"),
        name: "Arbitrary display name".to_owned(),
        kind,
        host: "coreaudio".to_owned(),
        manufacturer: None,
        driver: None,
        supports_input: true,
        supports_output: true,
        is_default_input: false,
        is_default_output: false,
        default_input_format: Some(format.clone()),
        default_output_format: Some(format),
    }
}

fn request() -> RouteConfig {
    RouteConfig {
        id: RouteId::new("virtual-mic").expect("ID"),
        source: SourceEndpoint::PhysicalInput(DeviceId::new("coreaudio:microphone").expect("ID")),
        destination: DestinationEndpoint::VirtualMicrophone(
            VirtualEndpointId::new("macos:blackhole-2ch").expect("ID"),
        ),
        echo_cancellation: false,
        processing_mode: ProcessingMode::SpeechDenoise,
        target_latency_ms: NonZeroU16::new(40).expect("latency"),
        denoise: DenoiseSettings::default(),
    }
}

fn inventory() -> DeviceInventory {
    DeviceInventory {
        host: "coreaudio".to_owned(),
        devices: vec![
            device("coreaudio:microphone", DeviceKind::Physical),
            device("coreaudio:BlackHole2ch_UID", DeviceKind::Virtual),
        ],
    }
}

#[test]
fn physical_only_supports_duplex_hardware_and_rejects_virtual_routes() {
    let router = PhysicalOnlyRouter;
    assert!(!router.capabilities().virtual_microphone);
    assert!(router.discover_endpoints(&inventory()).is_empty());
    assert_eq!(
        router.prepare_route(&request(), &inventory()),
        Err(RoutingError::UnsupportedPlatform)
    );
    let mut request = request();
    request.destination =
        DestinationEndpoint::PhysicalOutput(DeviceId::new("coreaudio:microphone").expect("ID"));
    assert!(router.prepare_route(&request, &inventory()).is_ok());
    router.release_route(&request.id).expect("release");
    router
        .release_route(&request.id)
        .expect("idempotent release");
    request.destination = DestinationEndpoint::PhysicalOutput(
        DeviceId::new("coreaudio:BlackHole2ch_UID").expect("ID"),
    );
    assert_eq!(
        router.prepare_route(&request, &inventory()),
        Err(RoutingError::UnsupportedEndpoint)
    );
}

#[test]
fn ancestry_rejects_shared_paths_and_unknown_graphs() {
    let path = DeviceId::new("shared-path").expect("ID");
    let destination = EndpointAncestry {
        recursive_paths: vec![path.clone()],
        complete: true,
    };
    let aggregate = EndpointAncestry {
        recursive_paths: vec![path],
        complete: false,
    };
    assert_eq!(
        validate_ancestry(&aggregate, &destination),
        Err(RoutingError::FeedbackLoop)
    );
    assert_eq!(
        validate_ancestry(&EndpointAncestry::default(), &destination),
        Err(RoutingError::UnverifiedSource)
    );
}

#[test]
fn incoming_pipewire_pair_is_separate_from_microphone_and_never_captures_render() {
    use audio_protocol::{EndpointAvailability, LoopbackEndpointId};
    use platform_routing::{incoming::*, linux::LinuxRouter};
    let mut graph = DeviceInventory {
        host: "pipewire".to_owned(),
        devices: vec![
            device(PIPEWIRE_INCOMING_PLAYBACK, DeviceKind::Virtual),
            device(PIPEWIRE_INCOMING_CAPTURE, DeviceKind::Virtual),
            device("pipewire:headphones", DeviceKind::Physical),
        ],
    };
    for (index, device) in graph.devices.iter_mut().enumerate() {
        device.host = "pipewire".to_owned();
        device.supports_input = index == 1;
        device.supports_output = index != 1;
    }
    let mut route = request();
    route.source =
        SourceEndpoint::SystemLoopback(LoopbackEndpointId::new(PIPEWIRE_INCOMING_ID).expect("ID"));
    route.destination = DestinationEndpoint::PhysicalOutput(graph.devices[2].id.clone());
    assert_eq!(
        linux_endpoint(&graph).availability,
        EndpointAvailability::Ready
    );
    assert_eq!(
        LinuxRouter.endpoint(&graph).availability,
        EndpointAvailability::Missing
    );
    let prepared = LinuxRouter
        .prepare_route(&route, &graph)
        .expect("incoming route");
    assert_eq!(prepared.input_device_id.as_str(), PIPEWIRE_INCOMING_CAPTURE);
    assert_eq!(prepared.output_device_id.as_str(), "pipewire:headphones");
    for kind in [
        DeviceKind::Virtual,
        DeviceKind::Aggregate,
        DeviceKind::Unknown,
    ] {
        let mut invalid = graph.clone();
        invalid.devices[2].kind = kind;
        assert_eq!(
            LinuxRouter.prepare_route(&route, &invalid),
            Err(RoutingError::UnverifiedDestination)
        );
    }
    let mut duplicate = graph.clone();
    duplicate.devices.push(graph.devices[0].clone());
    assert_eq!(
        LinuxRouter.prepare_route(&route, &duplicate),
        Err(RoutingError::InvalidEndpoint)
    );
    let mut missing = graph.clone();
    missing.devices.remove(1);
    assert_eq!(
        LinuxRouter.prepare_route(&route, &missing),
        Err(RoutingError::InvalidEndpoint)
    );
    route.destination = DestinationEndpoint::PhysicalOutput(graph.devices[0].id.clone());
    graph.devices[0].kind = DeviceKind::Physical;
    assert_eq!(
        LinuxRouter.prepare_route(&route, &graph),
        Err(RoutingError::FeedbackLoop)
    );
}

#[cfg(feature = "macos-blackhole")]
#[test]
fn blackhole_incoming_recapture_and_cross_route_conflicts_fail_closed() {
    use platform_routing::{incoming::validate_concurrent, macos::MacOsRouter};
    let mut incoming = request();
    incoming.source = SourceEndpoint::SystemLoopback(
        audio_protocol::LoopbackEndpointId::new("macos:blackhole-2ch").expect("ID"),
    );
    incoming.destination =
        DestinationEndpoint::PhysicalOutput(DeviceId::new("coreaudio:microphone").expect("ID"));
    let prepared = MacOsRouter
        .prepare_route(&incoming, &inventory())
        .expect("incoming prepared");
    assert_eq!(
        prepared.input_device_id.as_str(),
        "coreaudio:BlackHole2ch_UID"
    );
    assert_eq!(
        validate_concurrent(&incoming, &request()),
        Err(RoutingError::FeedbackLoop)
    );
    assert_eq!(
        validate_concurrent(&request(), &incoming),
        Err(RoutingError::FeedbackLoop)
    );
    assert_eq!(
        validate_concurrent(&incoming, &incoming),
        Err(RoutingError::EndpointInUse)
    );
    for id in ["coreaudio:BlackHole2ch_UID", "coreaudio:BlackHole2ch_2_UID"] {
        let mut graph = inventory();
        if id.ends_with("_2_UID") {
            graph.devices.push(device(id, DeviceKind::Physical));
        }
        incoming.destination = DestinationEndpoint::PhysicalOutput(DeviceId::new(id).expect("ID"));
        assert_eq!(
            MacOsRouter.prepare_route(&incoming, &graph),
            Err(RoutingError::FeedbackLoop)
        );
    }
}

mod linux {
    use super::*;
    use audio_protocol::EndpointAvailability;
    use platform_routing::linux::{LinuxRouter, PIPEWIRE_CAPTURE_ID, PIPEWIRE_PLAYBACK_ID};

    fn pair() -> (RouteConfig, DeviceInventory) {
        let host = "pipewire";
        let (output, input) = (PIPEWIRE_PLAYBACK_ID, PIPEWIRE_CAPTURE_ID);
        let mut inventory = DeviceInventory {
            host: host.to_owned(),
            devices: vec![
                device(&format!("{host}:microphone"), DeviceKind::Physical),
                device(output, DeviceKind::Virtual),
                device(input, DeviceKind::Virtual),
            ],
        };
        for (index, device) in inventory.devices.iter_mut().enumerate() {
            device.host = host.to_owned();
            device.supports_input = index != 1;
            device.supports_output = index == 1;
        }
        let mut route = request();
        route.source = SourceEndpoint::PhysicalInput(inventory.devices[0].id.clone());
        route.destination = DestinationEndpoint::VirtualMicrophone(
            VirtualEndpointId::new(LinuxRouter.endpoint_id()).expect("endpoint"),
        );
        (route, inventory)
    }

    #[test]
    fn paired_routes_keep_playback_and_recording_roles_separate() {
        let router = LinuxRouter;
        let (request, inventory) = pair();
        let endpoint = router.endpoint(&inventory);
        assert_eq!(endpoint.availability, EndpointAvailability::Ready);
        assert_eq!(
            endpoint.playback_device_id,
            Some(inventory.devices[1].id.clone())
        );
        assert_eq!(
            endpoint.capture_device_id,
            Some(inventory.devices[2].id.clone())
        );
        assert_eq!(endpoint.ancestry.recursive_paths.len(), 2);
        assert!(endpoint.can_create);
        let route = router.prepare_route(&request, &inventory).expect("prepare");
        assert_eq!(route.output_device_id, inventory.devices[1].id);
        assert_eq!(route.input_device_id, inventory.devices[0].id);
        assert!(!router.capabilities().requires_external_driver);
    }

    #[test]
    fn default_aliases_cannot_be_stored_as_physical_or_virtual_route_devices() {
        let (mut request, mut inventory) = pair();
        let headphones = device("pipewire:headphones", DeviceKind::Physical);
        request.destination = DestinationEndpoint::PhysicalOutput(headphones.id.clone());
        inventory.devices.push(headphones);
        let original = LinuxRouter
            .prepare_route(&request, &inventory)
            .expect("concrete route");
        // Defaults moving must not change the concrete IDs in a prepared/recovered route.
        inventory.devices.reverse();
        for device in &mut inventory.devices {
            device.is_default_input = true;
            device.is_default_output = true;
        }
        assert_eq!(
            LinuxRouter.prepare_route(&request, &inventory),
            Ok(original)
        );
        for kind in [DeviceKind::Unknown, DeviceKind::Physical] {
            for alias in ["pipewire:input_default", "pipewire:output_default"] {
                inventory.devices.push(device(alias, kind));
                let mut aliased = request.clone();
                if alias == "pipewire:input_default" {
                    aliased.source = SourceEndpoint::PhysicalInput(DeviceId::new(alias).unwrap());
                } else {
                    aliased.destination =
                        DestinationEndpoint::PhysicalOutput(DeviceId::new(alias).unwrap());
                }
                assert_eq!(
                    LinuxRouter.prepare_route(&aliased, &inventory),
                    Err(RoutingError::DynamicDeviceAlias)
                );
                // Even fallback physical-only policy must not bypass the identity check.
                assert_eq!(
                    PhysicalOnlyRouter.prepare_route(&aliased, &inventory),
                    Err(RoutingError::DynamicDeviceAlias)
                );
                if alias == "pipewire:input_default" {
                    aliased.destination = pair().0.destination;
                    assert_eq!(
                        LinuxRouter.validate_source(&aliased, &inventory),
                        Err(RoutingError::DynamicDeviceAlias)
                    );
                }
                inventory.devices.pop();
            }
        }
        inventory
            .devices
            .retain(|device| device.id.as_str() != "pipewire:headphones");
        assert_eq!(
            LinuxRouter.prepare_route(&request, &inventory),
            Err(RoutingError::DeviceUnavailable)
        );
    }

    #[test]
    fn ambiguous_incomplete_or_wrong_format_pairs_fail_closed() {
        let router = LinuxRouter;
        let (request, inventory) = pair();
        for scenario in 0..5 {
            let mut invalid = inventory.clone();
            match scenario {
                0 => {
                    invalid.devices.pop();
                }
                1 => invalid.devices.push(invalid.devices[1].clone()),
                2 => invalid.devices[2].supports_output = true,
                3 => {
                    invalid.devices[1]
                        .default_output_format
                        .as_mut()
                        .unwrap()
                        .channels = 8.try_into().unwrap()
                }
                _ => invalid.devices[2].host = "wrong-host".to_owned(),
            }
            assert_eq!(
                router.endpoint(&invalid).availability,
                EndpointAvailability::Invalid
            );
            assert_eq!(
                router.prepare_route(&request, &invalid),
                Err(RoutingError::InvalidEndpoint)
            );
        }
        let mut missing = inventory.clone();
        missing.devices.truncate(1);
        assert_eq!(
            router.endpoint(&missing).availability,
            EndpointAvailability::Missing
        );
        assert_eq!(
            router.prepare_route(&request, &missing),
            Err(RoutingError::MissingEndpoint)
        );
        assert!(
            router.validate_source(&request, &missing).is_ok(),
            "creation may validate a physical source before nodes exist"
        );
        missing.host = "alsa".to_owned();
        assert_eq!(
            router.endpoint(&missing).availability,
            EndpointAvailability::Unsupported
        );
        assert!(!router.endpoint(&missing).can_create);
    }

    #[test]
    fn capture_side_cannot_feed_its_own_playback_and_unknown_inputs_are_rejected() {
        let router = LinuxRouter;
        let (mut request, mut inventory) = pair();
        for kind in [
            DeviceKind::Virtual,
            DeviceKind::Aggregate,
            DeviceKind::Unknown,
        ] {
            inventory.devices[0].kind = kind;
            assert_eq!(
                router.prepare_route(&request, &inventory),
                Err(RoutingError::UnverifiedSource)
            );
        }
        request.source = SourceEndpoint::PhysicalInput(inventory.devices[2].id.clone());
        inventory.devices[2].kind = DeviceKind::Physical;
        assert_eq!(
            router.prepare_route(&request, &inventory),
            Err(RoutingError::FeedbackLoop)
        );
        request.destination = DestinationEndpoint::PhysicalOutput(inventory.devices[1].id.clone());
        assert_eq!(
            router.prepare_route(&request, &inventory),
            Err(RoutingError::FeedbackLoop)
        );
    }
}

#[cfg(feature = "macos-blackhole")]
mod blackhole {
    use super::*;
    use audio_protocol::EndpointAvailability;
    use platform_routing::macos::MacOsRouter;

    #[test]
    fn discovery_uses_identity_and_native_roles_instead_of_display_names() {
        let router = MacOsRouter;
        assert!(router.capabilities().virtual_microphone);
        assert!(router.capabilities().requires_external_driver);
        let mut devices = inventory();
        let endpoint = router.discover_endpoints(&devices).remove(0);
        assert_eq!(endpoint.availability, EndpointAvailability::Ready);
        assert_eq!(endpoint.capture_device_id, endpoint.playback_device_id);
        assert!(
            endpoint
                .instructions
                .iter()
                .any(|text| text.contains("BlackHole 2ch as the microphone"))
        );
        devices.devices[1].id = DeviceId::new("coreaudio:impostor").expect("ID");
        devices.devices[1].name = "BlackHole 2ch".to_owned();
        assert_eq!(
            router.discover_endpoints(&devices)[0].availability,
            EndpointAvailability::Missing
        );
        devices = inventory();
        devices.devices.push(devices.devices[1].clone());
        assert_eq!(
            router.discover_endpoints(&devices)[0].availability,
            EndpointAvailability::Invalid
        );
        for kind in [
            DeviceKind::Physical,
            DeviceKind::Aggregate,
            DeviceKind::Unknown,
        ] {
            devices = inventory();
            devices.devices[1].kind = kind;
            assert_eq!(
                router.prepare_route(&request(), &devices),
                Err(RoutingError::InvalidEndpoint)
            );
        }
        devices = inventory();
        devices.devices[1].supports_input = false;
        assert_eq!(
            router.prepare_route(&request(), &devices),
            Err(RoutingError::InvalidEndpoint)
        );
        devices = inventory();
        devices.devices[1]
            .default_output_format
            .as_mut()
            .expect("format")
            .channels = 16.try_into().expect("channels");
        assert_eq!(
            router.prepare_route(&request(), &devices),
            Err(RoutingError::InvalidEndpoint)
        );
        devices = inventory();
        devices.host = "pipewire".to_owned();
        assert_eq!(
            router.discover_endpoints(&devices)[0].availability,
            EndpointAvailability::Unsupported
        );
        assert_eq!(
            router.prepare_route(&request(), &devices),
            Err(RoutingError::UnsupportedPlatform)
        );
    }

    #[test]
    fn exact_endpoint_resolution_survives_restart_and_never_falls_back() {
        let config = request();
        let mut devices = inventory();
        let prepared = MacOsRouter
            .prepare_route(&config, &devices)
            .expect("valid route");
        assert_eq!(
            prepared.output_device_id.as_str(),
            "coreaudio:BlackHole2ch_UID"
        );
        for _ in 0..100 {
            MacOsRouter.release_route(&config.id).expect("release");
            devices.devices.reverse();
            for device in &mut devices.devices {
                device.is_default_output = !device.is_default_output;
            }
            assert_eq!(
                MacOsRouter
                    .prepare_route(&config, &devices)
                    .expect("recreated router"),
                prepared
            );
        }
        devices
            .devices
            .retain(|device| device.kind == DeviceKind::Physical);
        assert_eq!(
            MacOsRouter.prepare_route(&config, &devices),
            Err(RoutingError::MissingEndpoint)
        );
        assert!(
            RoutingError::MissingEndpoint
                .to_dto()
                .suggested_action
                .expect("instructions")
                .contains("setup instructions")
        );
        assert!(MacOsRouter.prepare_route(&config, &inventory()).is_ok());
        devices = inventory();
        devices.devices.remove(0);
        assert_eq!(
            MacOsRouter.prepare_route(&config, &devices),
            Err(RoutingError::DeviceUnavailable)
        );
    }

    #[test]
    fn direct_mirrored_and_disguised_feedback_routes_are_rejected() {
        let mut devices = inventory();
        devices
            .devices
            .push(device("coreaudio:BlackHole2ch_2_UID", DeviceKind::Virtual));
        for id in ["coreaudio:BlackHole2ch_UID", "coreaudio:BlackHole2ch_2_UID"] {
            let mut request = request();
            request.source = SourceEndpoint::PhysicalInput(DeviceId::new(id).expect("ID"));
            assert_eq!(
                MacOsRouter.prepare_route(&request, &devices),
                Err(RoutingError::FeedbackLoop)
            );
            request.destination = DestinationEndpoint::PhysicalOutput(
                DeviceId::new("coreaudio:BlackHole2ch_UID").expect("ID"),
            );
            assert_eq!(
                MacOsRouter.prepare_route(&request, &devices),
                Err(RoutingError::FeedbackLoop)
            );
        }
        let mut request = request();
        request.source = SourceEndpoint::VirtualRouteInput(
            VirtualEndpointId::new("macos:blackhole-2ch").expect("ID"),
        );
        assert_eq!(
            MacOsRouter.prepare_route(&request, &devices),
            Err(RoutingError::FeedbackLoop)
        );
    }

    #[test]
    fn unsupported_and_unverifiable_graphs_fail_closed() {
        for kind in [
            DeviceKind::Unknown,
            DeviceKind::Aggregate,
            DeviceKind::Virtual,
        ] {
            let mut devices = inventory();
            devices.devices[0].kind = kind;
            assert_eq!(
                MacOsRouter.prepare_route(&request(), &devices),
                Err(RoutingError::UnverifiedSource)
            );
        }
        let mut request = request();
        request.destination = DestinationEndpoint::VirtualMicrophone(
            VirtualEndpointId::new("macos:blackhole-16ch").expect("ID"),
        );
        assert_eq!(
            MacOsRouter.prepare_route(&request, &inventory()),
            Err(RoutingError::UnsupportedEndpoint)
        );
    }
}
