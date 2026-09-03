use std::num::NonZeroU16;

use audio_protocol::{
    AudioFormat, BufferSizeFrames, DenoiseSettings, DestinationEndpoint, EngineErrorDto,
    EngineEvent, ErrorCode, PipelineMetrics, PipelineState, ProcessingMode, RouteConfig, RouteId,
    SampleFormat, SampleRateHz, SourceEndpoint, VirtualEndpointId,
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
    round_trip(&PipelineState::Recovering);

    let event = EngineEvent::Metrics {
        route_id: route.id,
        metrics: PipelineMetrics {
            input_peak: 0.75,
            output_rms: 0.25,
            inference_p99_us: 3_500,
            input_ring_high_water_frames: 2_400,
            drift_correction_ppm: -42,
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
fn invalid_identifier_cannot_enter_through_json() {
    let decoded = serde_json::from_str::<RouteId>("\" \\n \"");
    assert!(decoded.is_err());
}
