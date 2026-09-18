use std::num::NonZeroU16;

use audio_protocol::{
    AudioDeviceDescriptor, AudioFormat, BufferSizeFrames, DenoiseSettings, DestinationEndpoint,
    DeviceId, DeviceInventory, EngineErrorDto, EngineEvent, EngineSnapshot, ErrorCode, ModelInfo,
    PipelineMetrics, PipelineState, ProcessingMode, RouteConfig, RouteId, RouteSnapshot,
    SampleFormat, SampleRateHz, SourceEndpoint, StreamDiagnostics, VirtualEndpointId,
};
use serde::{Serialize, de::DeserializeOwned};

fn round_trip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_string(value).expect("value serializes");
    let decoded: T = serde_json::from_str(&json).expect("value deserializes");
    assert_eq!(&decoded, value, "wire value: {json}");
}

#[test]
fn dto_safe_domain_graph_round_trips() {
    let route = RouteConfig {
        id: RouteId::new("route:monitor").expect("valid route ID"),
        source: SourceEndpoint::PhysicalInput(
            "coreaudio:input".try_into().expect("valid device ID"),
        ),
        destination: DestinationEndpoint::VirtualMicrophone(
            VirtualEndpointId::new("virtual:microphone").expect("valid endpoint ID"),
        ),
        echo_cancellation: false,
        processing_mode: ProcessingMode::SpeechDenoise,
        target_latency_ms: NonZeroU16::new(60).expect("non-zero latency"),
        denoise: DenoiseSettings {
            enabled: true,
            attenuation_limit_db: Some(35.0),
            post_filter_enabled: true,
        },
    };
    round_trip(&route);
    round_trip(&AudioFormat {
        sample_rate_hz: SampleRateHz::new(48_000).expect("valid rate"),
        channels: 2_u16.try_into().expect("valid channels"),
        sample_format: SampleFormat::F32,
        buffer_size_frames: Some(BufferSizeFrames::new(480).expect("valid buffer size")),
    });
    round_trip(&DeviceInventory {
        host: "coreaudio".to_owned(),
        devices: vec![AudioDeviceDescriptor {
            kind: audio_protocol::DeviceKind::Physical,
            id: DeviceId::new("coreaudio:input").expect("valid device ID"),
            name: "Built-in Microphone".to_owned(),
            host: "coreaudio".to_owned(),
            manufacturer: Some("Example".to_owned()),
            driver: None,
            supports_input: true,
            supports_output: false,
            is_default_input: true,
            is_default_output: false,
            default_input_format: Some(AudioFormat {
                sample_rate_hz: SampleRateHz::new(48_000).expect("valid rate"),
                channels: 1_u16.try_into().expect("valid channels"),
                sample_format: SampleFormat::F32,
                buffer_size_frames: None,
            }),
            default_output_format: None,
        }],
    });
    round_trip(&PipelineState::Recovering);

    let event = EngineEvent::Metrics {
        route_id: route.id,
        metrics: PipelineMetrics {
            input_peak: 0.75,
            output_rms: 0.25,
            inference_p99_us: 3_500,
            input_ring_high_water_frames: 2_400,
            drift_correction_ppm: -42,
            degraded: true,
            overload_events: 2,
            skipped_inferences: 17,
            estimated_latency_ms: 48.5,
            ..PipelineMetrics::default()
        },
    };
    round_trip(&event);
}

#[test]
fn every_stable_error_code_matches_its_wire_string() {
    let codes = [
        ErrorCode::InvalidIdentifier,
        ErrorCode::InvalidConfiguration,
        ErrorCode::InvalidState,
        ErrorCode::CommandQueueFull,
        ErrorCode::DeviceNotFound,
        ErrorCode::DeviceDisconnected,
        ErrorCode::UnsupportedFormat,
        ErrorCode::AudioIo,
        ErrorCode::Dsp,
        ErrorCode::Denoise,
        ErrorCode::ModelAssetMissing,
        ErrorCode::ModelHashMismatch,
        ErrorCode::ModelLoadFailed,
        ErrorCode::InvalidModelFrame,
        ErrorCode::NonFiniteAudio,
        ErrorCode::StreamError,
        ErrorCode::Routing,
        ErrorCode::WorkerTerminated,
        ErrorCode::ShutdownInProgress,
        ErrorCode::Internal,
    ];
    for code in codes {
        let json = serde_json::to_string(&code).expect("code serializes");
        assert_eq!(json, format!("\"{}\"", code.as_str()));
        round_trip(&code);
    }

    round_trip(
        &EngineErrorDto::new(ErrorCode::UnsupportedFormat, "Format is unavailable")
            .with_details("48 kHz mono required")
            .with_suggested_action("Choose another device"),
    );
}

#[test]
fn new_pipeline_health_fields_default_for_older_payloads() {
    let mut encoded = serde_json::to_value(PipelineMetrics::default()).expect("metrics serialize");
    let object = encoded
        .as_object_mut()
        .expect("metrics encode as an object");
    object.remove("degraded");
    object.remove("overloadEvents");
    object.remove("skippedInferences");
    object.remove("resetLastUs");
    object.remove("resetTotalUs");

    let decoded: PipelineMetrics = serde_json::from_value(encoded).expect("older metrics decode");
    assert!(!decoded.degraded);
    assert_eq!(decoded.overload_events, 0);
    assert_eq!(decoded.skipped_inferences, 0);
    assert_eq!(decoded.reset_last_us, 0);
    assert_eq!(decoded.reset_total_us, 0);
}

#[test]
fn invalid_identifier_cannot_enter_through_json() {
    let decoded = serde_json::from_str::<RouteId>("\" \\n \"");
    assert!(decoded.is_err());
}

#[test]
fn engine_snapshot_round_trip_contains_no_pcm_payload() {
    let config = RouteConfig {
        id: RouteId::new("route:desktop").expect("valid route ID"),
        source: SourceEndpoint::PhysicalInput(
            DeviceId::new("coreaudio:input").expect("valid input ID"),
        ),
        destination: DestinationEndpoint::PhysicalOutput(
            DeviceId::new("coreaudio:output").expect("valid output ID"),
        ),
        echo_cancellation: false,
        processing_mode: ProcessingMode::SpeechDenoise,
        target_latency_ms: NonZeroU16::new(40).expect("non-zero latency"),
        denoise: DenoiseSettings::default(),
    };
    let snapshot = EngineSnapshot {
        routing: Default::default(),
        revision: 7,
        inventory: DeviceInventory {
            host: "coreaudio".to_owned(),
            devices: Vec::new(),
        },
        routes: vec![RouteSnapshot {
            model: Some(ModelInfo {
                id: "test-model".to_owned(),
                sample_rate_hz: SampleRateHz::new(48_000).expect("valid rate"),
                channels: 1,
                frame_samples_per_channel: 480,
                algorithmic_latency_samples: 960,
            }),
            input_stream: StreamDiagnostics {
                callbacks: 10,
                frames: 4_800,
                ..StreamDiagnostics::default()
            },
            ..RouteSnapshot::pending(config, PipelineState::Running)
        }],
        ..EngineSnapshot::default()
    };

    round_trip(&snapshot);
    let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
    assert!(!json.contains("samples"));
    assert!(!json.contains("pcm"));
}

#[test]
fn routing_metadata_round_trips_and_older_snapshots_fail_closed() {
    use audio_protocol::{
        EndpointAncestry, EndpointAvailability, PlatformEndpoint, RoutingCapabilities,
        RoutingSnapshot,
    };
    let routing = RoutingSnapshot {
        loopback_endpoints: Vec::new(),
        capabilities: RoutingCapabilities {
            physical_input: true,
            physical_output: true,
            virtual_microphone: true,
            requires_external_driver: true,
            ..Default::default()
        },
        endpoints: vec![PlatformEndpoint {
            id: VirtualEndpointId::new("macos:openkrisp-microphone").expect("ID"),
            name: "OpenKrisp Microphone".to_owned(),
            availability: EndpointAvailability::Ready,
            can_create: false,
            playback_device_id: Some(
                DeviceId::new("coreaudio:OpenKrispMicrophone_UID").expect("ID"),
            ),
            capture_device_id: Some(
                DeviceId::new("coreaudio:OpenKrispMicrophone_UID").expect("ID"),
            ),
            ancestry: EndpointAncestry {
                recursive_paths: vec![
                    DeviceId::new("coreaudio:OpenKrispMicrophone_UID").expect("ID"),
                ],
                complete: true,
            },
            instructions: vec!["Select OpenKrisp Microphone in the receiving app.".to_owned()],
            setup_url: "https://github.com/saeta-eth/OpenKrisp/blob/main/docs/INSTALLATION.md"
                .to_owned(),
        }],
    };
    round_trip(&routing);
    let mut old = serde_json::to_value(EngineSnapshot::default()).expect("snapshot");
    old.as_object_mut().expect("object").remove("routing");
    let decoded: EngineSnapshot = serde_json::from_value(old).expect("older snapshot");
    assert!(!decoded.routing.capabilities.virtual_microphone);
    assert!(decoded.routing.endpoints.is_empty());
}

#[test]
fn legacy_route_defaults_enable_aec_without_claiming_active_processing() {
    let value = serde_json::json!({
        "id": "mic", "source": {"physicalInput": "mic:physical"},
        "destination": {"physicalOutput": "out:physical"},
        "processingMode": "aligned_bypass", "targetLatencyMs": 40,
        "denoise": {"enabled": false, "attenuationLimitDb": null, "postFilterEnabled": false}
    });
    let config: RouteConfig = serde_json::from_value(value.clone()).unwrap();
    assert!(config.echo_cancellation);
    let mut snapshot =
        serde_json::to_value(RouteSnapshot::pending(config, PipelineState::Running)).unwrap();
    snapshot.as_object_mut().unwrap().remove("echoCancellation");
    let restored: RouteSnapshot = serde_json::from_value(snapshot).unwrap();
    assert!(
        restored.echo_cancellation.is_none(),
        "no telemetry does not mean active AEC"
    );
    let mut disabled = value;
    disabled["echoCancellation"] = serde_json::json!(false);
    assert!(
        !serde_json::from_value::<RouteConfig>(disabled)
            .unwrap()
            .echo_cancellation
    );
}
