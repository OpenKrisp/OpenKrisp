use std::time::Duration;

use audio_engine::{
    CallbackSchedule, FakeAudioSink, FakeAudioSource, SimulatedRoute, bounded_audio_ring,
};
use audio_protocol::{ChannelCount, DenoiseSettings, SampleRateHz};
use denoise_core::{
    FakeFailureMode, FakeNoiseReducer, FakeNoiseReducerConfig, NoiseReducerDescriptor,
    NoiseReductionStats,
};

#[test]
fn ring_slice_io_performs_no_steady_state_allocations() {
    let channels = ChannelCount::new(2).expect("valid channel count");
    let (mut producer, mut consumer) = bounded_audio_ring(256, channels).expect("valid ring");
    let input = [0.25; 128];
    let mut output = [0.0; 128];

    let allocations = allocation_counter::measure(|| {
        for _ in 0..100 {
            let written = producer.write(&input).expect("aligned input");
            assert_eq!(written.written_frames, 64);
            let read = consumer.read(&mut output).expect("aligned output");
            assert_eq!(read.read_frames, 64);
        }
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}

#[test]
fn simulated_route_performs_no_steady_state_allocations() {
    let channels = ChannelCount::new(1).expect("valid channel count");
    let rate = SampleRateHz::new(48_000).expect("valid rate");
    let source = FakeAudioSource::new(
        rate,
        channels,
        100,
        CallbackSchedule::without_jitter(vec![64, 96, 240, 480]).expect("valid schedule"),
        Vec::new(),
    )
    .expect("valid source");
    let sink = FakeAudioSink::new(
        rate,
        channels,
        0,
        CallbackSchedule::without_jitter(vec![96, 128, 480]).expect("valid schedule"),
        Vec::new(),
        4_800,
    )
    .expect("valid sink");
    let reducer = FakeNoiseReducer::new(FakeNoiseReducerConfig {
        descriptor: NoiseReducerDescriptor {
            model_id: "allocation-test".to_owned(),
            sample_rate_hz: rate,
            channels,
            frame_samples_per_channel: 480,
            algorithmic_latency_samples: 480,
        },
        settings: DenoiseSettings::default(),
        enabled_gain: 0.5,
        processing_delay: Duration::ZERO,
        failure_mode: FakeFailureMode::Never,
        stats: NoiseReductionStats::default(),
    })
    .expect("valid reducer");
    let mut route =
        SimulatedRoute::new(source, sink, reducer, 4_800, 4_800, 2_400, 960).expect("valid route");
    route.start().expect("route starts");

    let allocations = allocation_counter::measure(|| {
        route.run_for_ns(100_000_000).expect("simulation succeeds");
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}
