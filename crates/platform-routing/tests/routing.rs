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
            VirtualEndpointId::new("macos:openkrisp-microphone").expect("ID"),
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
            device("coreaudio:OpenKrispMicrophone_UID", DeviceKind::Virtual),
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
        DeviceId::new("coreaudio:OpenKrispMicrophone_UID").expect("ID"),
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

#[cfg(feature = "macos-virtual-devices")]
#[test]
fn microphone_recapture_and_cross_route_conflicts_fail_closed() {
    use platform_routing::{incoming::validate_concurrent, macos::MacOsRouter};
    let mut incoming = request();
    incoming.source = SourceEndpoint::SystemLoopback(
        audio_protocol::LoopbackEndpointId::new("macos:openkrisp-microphone").expect("ID"),
    );
    incoming.destination =
        DestinationEndpoint::PhysicalOutput(DeviceId::new("coreaudio:microphone").expect("ID"));
    assert_eq!(
        MacOsRouter.prepare_route(&incoming, &inventory()),
        Err(RoutingError::UnsupportedEndpoint)
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
    for id in [
        "coreaudio:OpenKrispMicrophone_UID",
        "coreaudio:OpenKrispMicrophone_2_UID",
    ] {
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

#[cfg(feature = "macos-virtual-devices")]
mod macos_microphone {
    use super::*;
    use audio_protocol::EndpointAvailability;
    use platform_routing::macos::MacOsRouter;

    #[test]
    fn discovery_uses_identity_and_native_roles_instead_of_display_names() {
        let router = MacOsRouter;
        assert!(router.capabilities().virtual_microphone);
        assert!(router.capabilities().requires_external_driver);
        let mut devices = inventory();
        let endpoints = router.discover_endpoints(&devices);
        assert_eq!(endpoints.len(), 1);
        let endpoint = &endpoints[0];
        assert_eq!(endpoint.name, "OpenKrisp Microphone");
        assert_eq!(endpoint.availability, EndpointAvailability::Ready);
        assert_eq!(endpoint.capture_device_id, endpoint.playback_device_id);
        assert!(
            endpoint
                .instructions
                .iter()
                .any(|text| text.contains("OpenKrisp Microphone as the microphone"))
        );
        devices.devices[1].id = DeviceId::new("coreaudio:impostor").expect("ID");
        devices.devices[1].name = "OpenKrisp Microphone".to_owned();
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
            "coreaudio:OpenKrispMicrophone_UID"
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
        devices.devices.push(device(
            "coreaudio:OpenKrispMicrophone_2_UID",
            DeviceKind::Virtual,
        ));
        for id in [
            "coreaudio:OpenKrispMicrophone_UID",
            "coreaudio:OpenKrispMicrophone_2_UID",
        ] {
            let mut request = request();
            request.source = SourceEndpoint::PhysicalInput(DeviceId::new(id).expect("ID"));
            assert_eq!(
                MacOsRouter.prepare_route(&request, &devices),
                Err(RoutingError::FeedbackLoop)
            );
            request.destination = DestinationEndpoint::PhysicalOutput(
                DeviceId::new("coreaudio:OpenKrispMicrophone_UID").expect("ID"),
            );
            assert_eq!(
                MacOsRouter.prepare_route(&request, &devices),
                Err(RoutingError::FeedbackLoop)
            );
        }
        let mut request = request();
        request.source = SourceEndpoint::VirtualRouteInput(
            VirtualEndpointId::new("macos:openkrisp-microphone").expect("ID"),
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
            VirtualEndpointId::new("macos:openkrisp-speaker").expect("ID"),
        );
        assert_eq!(
            MacOsRouter.prepare_route(&request, &inventory()),
            Err(RoutingError::UnsupportedEndpoint)
        );
    }
}

#[cfg(feature = "macos-virtual-devices")]
mod macos_call_devices {
    use super::*;
    use audio_protocol::{EndpointAvailability, LoopbackEndpointId};
    use platform_routing::{incoming::validate_concurrent, macos::*};

    fn pair() -> (RouteConfig, DeviceInventory) {
        let mut devices = inventory();
        let mut incoming_device = device(SPEAKER_DEVICE_ID, DeviceKind::Virtual);
        incoming_device
            .default_input_format
            .as_mut()
            .unwrap()
            .channels = 16.try_into().unwrap();
        incoming_device
            .default_output_format
            .as_mut()
            .unwrap()
            .channels = 16.try_into().unwrap();
        devices.devices.push(incoming_device);
        let mut incoming = request();
        incoming.id = RouteId::new("incoming").unwrap();
        incoming.source =
            SourceEndpoint::SystemLoopback(LoopbackEndpointId::new(SPEAKER_ENDPOINT_ID).unwrap());
        incoming.destination =
            DestinationEndpoint::PhysicalOutput(DeviceId::new("coreaudio:microphone").unwrap());
        (incoming, devices)
    }

    #[test]
    fn distinct_buses_prepare_together_in_either_order_and_recover_independently() {
        let (incoming, mut devices) = pair();
        let microphone = request();
        let prepared_mic = MacOsRouter.prepare_route(&microphone, &devices).unwrap();
        let prepared_incoming = MacOsRouter.prepare_route(&incoming, &devices).unwrap();
        assert_eq!(prepared_mic.capture_stereo_channels, None);
        assert_eq!(prepared_incoming.capture_stereo_channels, Some(16));
        assert_eq!(
            prepared_incoming.input_device_id.as_str(),
            SPEAKER_DEVICE_ID
        );
        assert_eq!(prepared_mic.output_device_id.as_str(), MICROPHONE_DEVICE_ID);
        for (first, second) in [(&incoming, &microphone), (&microphone, &incoming)] {
            validate_concurrent(first, second).unwrap();
            MacOsRouter.release_route(&first.id).unwrap();
            assert!(MacOsRouter.prepare_route(second, &devices).is_ok());
        }
        for _ in 0..10 {
            devices.devices.reverse();
            for device in &mut devices.devices {
                device.is_default_input = !device.is_default_input;
                device.name = "Renamed device".into();
            }
            assert_eq!(
                MacOsRouter.prepare_route(&incoming, &devices).unwrap(),
                prepared_incoming
            );
        }
        let removed = devices
            .devices
            .iter()
            .position(|d| d.id.as_str() == SPEAKER_DEVICE_ID)
            .unwrap();
        let incoming_device = devices.devices.remove(removed);
        assert_eq!(
            MacOsRouter.prepare_route(&incoming, &devices),
            Err(RoutingError::MissingEndpoint)
        );
        assert_eq!(
            MacOsRouter.prepare_route(&microphone, &devices).unwrap(),
            prepared_mic
        );
        devices.devices.push(incoming_device);
        assert_eq!(
            MacOsRouter.prepare_route(&incoming, &devices).unwrap(),
            prepared_incoming
        );
        devices
            .devices
            .retain(|d| d.id.as_str() != MICROPHONE_DEVICE_ID);
        assert_eq!(
            MacOsRouter.prepare_route(&microphone, &devices),
            Err(RoutingError::MissingEndpoint)
        );
        assert_eq!(
            MacOsRouter.prepare_route(&incoming, &devices).unwrap(),
            prepared_incoming
        );
    }

    #[test]
    fn incoming_discovery_requires_exact_uid_transport_roles_and_channel_width() {
        let (incoming, devices) = pair();
        let endpoints = MacOsRouter.discover_loopback_endpoints(&devices);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].name, "OpenKrisp Speaker");
        assert_eq!(endpoints[0].id.as_str(), SPEAKER_ENDPOINT_ID);
        assert_eq!(endpoints[0].availability, EndpointAvailability::Ready);
        let mut duplicate = devices.clone();
        duplicate.devices.push(devices.devices[2].clone());
        assert_eq!(
            MacOsRouter.prepare_route(&incoming, &duplicate),
            Err(RoutingError::InvalidEndpoint)
        );
        for variant in 0..7 {
            let mut invalid = devices.clone();
            let d = &mut invalid.devices[2];
            match variant {
                0 => d.kind = DeviceKind::Physical,
                1 => d.kind = DeviceKind::Aggregate,
                2 => d.host = "pipewire".into(),
                3 => d.supports_input = false,
                4 => d.supports_output = false,
                5 => d.default_input_format.as_mut().unwrap().channels = 2.try_into().unwrap(),
                _ => d.default_output_format.as_mut().unwrap().channels = 2.try_into().unwrap(),
            }
            assert_eq!(
                MacOsRouter.prepare_route(&incoming, &invalid),
                Err(RoutingError::InvalidEndpoint)
            );
        }
        let mut impostor = devices.clone();
        impostor.devices[2].id = DeviceId::new("coreaudio:impostor").unwrap();
        impostor.devices[2].name = "OpenKrisp Speaker".into();
        assert_eq!(
            MacOsRouter.prepare_route(&incoming, &impostor),
            Err(RoutingError::MissingEndpoint)
        );
    }

    #[test]
    fn mirrors_and_disguised_virtual_devices_never_become_physical_destinations_or_microphones() {
        let (mut incoming, mut devices) = pair();
        for id in [
            MICROPHONE_DEVICE_ID,
            "coreaudio:OpenKrispMicrophone_2_UID",
            SPEAKER_DEVICE_ID,
            "coreaudio:OpenKrispSpeaker_2_UID",
        ] {
            devices.devices.retain(|d| d.id.as_str() != id);
            devices.devices.push(device(id, DeviceKind::Physical));
            incoming.destination = DestinationEndpoint::PhysicalOutput(DeviceId::new(id).unwrap());
            assert_eq!(
                MacOsRouter.prepare_route(&incoming, &devices),
                Err(RoutingError::FeedbackLoop)
            );
        }
        // A misclassified separate bus must not be accepted as the microphone's physical source.
        let (_, mut devices) = pair();
        devices.devices[2].kind = DeviceKind::Physical;
        let mut microphone = request();
        microphone.source =
            SourceEndpoint::PhysicalInput(DeviceId::new(SPEAKER_DEVICE_ID).unwrap());
        assert_eq!(
            MacOsRouter.prepare_route(&microphone, &devices),
            Err(RoutingError::UnverifiedSource)
        );
    }
}

#[cfg(feature = "macos-virtual-devices")]
mod named_devices {
    use super::*;
    use audio_protocol::{EndpointAvailability, LoopbackEndpointId};
    use platform_routing::macos::*;

    fn setup() -> (DeviceInventory, RouteConfig, RouteConfig) {
        let mut devices = inventory();
        let mut speaker = device(SPEAKER_DEVICE_ID, DeviceKind::Virtual);
        speaker.default_input_format.as_mut().unwrap().channels = 16.try_into().unwrap();
        speaker.default_output_format.as_mut().unwrap().channels = 16.try_into().unwrap();
        devices.devices.push(speaker);
        devices
            .devices
            .push(device("coreaudio:speakers", DeviceKind::Physical));
        let mut microphone = request();
        microphone.destination = DestinationEndpoint::VirtualMicrophone(
            VirtualEndpointId::new(MICROPHONE_ENDPOINT_ID).unwrap(),
        );
        let mut incoming = microphone.clone();
        incoming.id = RouteId::new("incoming").unwrap();
        incoming.source =
            SourceEndpoint::SystemLoopback(LoopbackEndpointId::new(SPEAKER_ENDPOINT_ID).unwrap());
        incoming.destination =
            DestinationEndpoint::PhysicalOutput(DeviceId::new("coreaudio:speakers").unwrap());
        (devices, microphone, incoming)
    }

    #[test]
    fn removed_endpoints_are_not_discovered_or_resolved_even_when_installed() {
        let (mut devices, mut microphone, mut incoming) = setup();
        for id in ["coreaudio:BlackHole2ch_UID", "coreaudio:BlackHole16ch_UID"] {
            devices.devices.push(device(id, DeviceKind::Virtual));
        }
        let snapshot = MacOsRouter.snapshot(&devices);
        assert_eq!(snapshot.endpoints.len(), 1);
        assert_eq!(snapshot.endpoints[0].id.as_str(), MICROPHONE_ENDPOINT_ID);
        assert_eq!(snapshot.loopback_endpoints.len(), 1);
        assert_eq!(
            snapshot.loopback_endpoints[0].id.as_str(),
            SPEAKER_ENDPOINT_ID
        );
        for id in ["macos:blackhole-2ch", "macos:blackhole-16ch"] {
            microphone.destination =
                DestinationEndpoint::VirtualMicrophone(VirtualEndpointId::new(id).unwrap());
            incoming.source = SourceEndpoint::SystemLoopback(LoopbackEndpointId::new(id).unwrap());
            assert_eq!(
                MacOsRouter.prepare_route(&microphone, &devices),
                Err(RoutingError::UnsupportedEndpoint)
            );
            assert_eq!(
                MacOsRouter.prepare_route(&incoming, &devices),
                Err(RoutingError::UnsupportedEndpoint)
            );
        }
        devices.devices.retain(|device| {
            !matches!(device.id.as_str(), MICROPHONE_DEVICE_ID | SPEAKER_DEVICE_ID)
        });
        let snapshot = MacOsRouter.snapshot(&devices);
        assert_eq!(
            snapshot.endpoints[0].availability,
            EndpointAvailability::Missing
        );
        assert_eq!(
            snapshot.loopback_endpoints[0].availability,
            EndpointAvailability::Missing
        );
    }

    #[test]
    fn named_devices_require_exact_native_identity_roles_and_width() {
        for (id, channels) in [(MICROPHONE_DEVICE_ID, 2), (SPEAKER_DEVICE_ID, 16)] {
            for invalid in 0..9 {
                let (mut devices, microphone, incoming) = setup();
                let index = devices
                    .devices
                    .iter()
                    .position(|d| d.id.as_str() == id)
                    .unwrap();
                let d = &mut devices.devices[index];
                let expected = match invalid {
                    0 => {
                        d.id = DeviceId::new("coreaudio:impostor").unwrap();
                        d.name = if channels == 2 {
                            "OpenKrisp Microphone"
                        } else {
                            "OpenKrisp Speaker"
                        }
                        .into();
                        RoutingError::MissingEndpoint
                    }
                    1 => {
                        d.kind = DeviceKind::Physical;
                        RoutingError::InvalidEndpoint
                    }
                    2 => {
                        d.host = "pipewire".into();
                        RoutingError::InvalidEndpoint
                    }
                    3 => {
                        d.supports_input = false;
                        RoutingError::InvalidEndpoint
                    }
                    4 => {
                        d.supports_output = false;
                        RoutingError::InvalidEndpoint
                    }
                    5 => {
                        d.default_input_format.as_mut().unwrap().channels = 1.try_into().unwrap();
                        RoutingError::InvalidEndpoint
                    }
                    6 => {
                        d.default_output_format.as_mut().unwrap().channels = 1.try_into().unwrap();
                        RoutingError::InvalidEndpoint
                    }
                    7 => {
                        d.kind = DeviceKind::Aggregate;
                        RoutingError::InvalidEndpoint
                    }
                    _ => {
                        let duplicate = d.clone();
                        devices.devices.push(duplicate);
                        RoutingError::InvalidEndpoint
                    }
                };
                let route = if channels == 2 {
                    &microphone
                } else {
                    &incoming
                };
                assert_eq!(MacOsRouter.prepare_route(route, &devices), Err(expected));
            }
        }
    }
}

#[test]
fn pipewire_speaker_name_matches_the_call_app_sink_without_changing_node_identities() {
    let inventory = DeviceInventory {
        host: "pipewire".into(),
        devices: vec![],
    };
    let endpoint = platform_routing::incoming::linux_endpoint(&inventory);
    assert_eq!(endpoint.name, "OpenKrisp Speaker");
    assert_eq!(endpoint.id.as_str(), "linux:pipewire-incoming");
    let args = platform_routing::pipewire::INCOMING_LOOPBACK_ARGS;
    let sink = args
        .iter()
        .find(|arg| arg.starts_with("--capture-props="))
        .unwrap();
    assert!(sink.contains("node.name=openkrisp_incoming_sink "));
    assert!(sink.contains("node.description=\"OpenKrisp Speaker\""));
    assert!(sink.contains("media.class=Audio/Sink "));
    assert!(
        args.iter()
            .any(|arg| arg.contains("node.name=openkrisp_incoming_source "))
    );
}
