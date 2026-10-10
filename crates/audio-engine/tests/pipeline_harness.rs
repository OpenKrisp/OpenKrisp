use audio_engine::{
    CallbackSchedule, FakeAudioSink, FakeAudioSource, PipelineWorkerConfig, SimulatedRoute,
};
use audio_protocol::{AudioFormat, ChannelCount, DenoiseSettings, SampleFormat, SampleRateHz};
use denoise_core::{
    FakeFailureMode, FakeNoiseReducer, FakeNoiseReducerConfig, NoiseReducerDescriptor,
    NoiseReductionStats,
};
use std::time::Duration;

fn format(rate: u32, channels: u16) -> AudioFormat {
    AudioFormat {
        sample_rate_hz: SampleRateHz::new(rate).unwrap(),
        channels: ChannelCount::new(channels).unwrap(),
        sample_format: SampleFormat::F32,
        buffer_size_frames: None,
    }
}
fn route(
    input_rate: u32,
    output_rate: u32,
    channels: u16,
    ppm: i32,
    irregular: bool,
) -> SimulatedRoute<FakeNoiseReducer> {
    let input = format(input_rate, channels);
    let output = format(output_rate, channels);
    let schedule = |rate: u32| {
        if irregular {
            CallbackSchedule::new(
                vec![64, 96, 128, 240, 256, 480, 512, 960, 1024],
                vec![0, -50_000, 80_000],
            )
            .unwrap()
        } else {
            CallbackSchedule::without_jitter(vec![rate as usize / 100]).unwrap()
        }
    };
    let source = FakeAudioSource::new(
        input.sample_rate_hz,
        input.channels,
        ppm,
        schedule(input_rate),
        vec![],
    )
    .unwrap();
    let sink = FakeAudioSink::new(
        output.sample_rate_hz,
        output.channels,
        0,
        schedule(output_rate),
        vec![],
        output_rate as usize * channels as usize,
    )
    .unwrap();
    let reducer = FakeNoiseReducer::new(FakeNoiseReducerConfig {
        descriptor: NoiseReducerDescriptor {
            model_id: "production-core-fixture".into(),
            sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
            channels: input.channels,
            frame_samples_per_channel: 480,
            algorithmic_latency_samples: 480,
        },
        settings: DenoiseSettings::default(),
        enabled_gain: 0.5,
        processing_delay: Duration::ZERO,
        failure_mode: FakeFailureMode::Never,
        stats: NoiseReductionStats::default(),
    })
    .unwrap();
    let mut config = PipelineWorkerConfig::new(input, output, input_rate as usize / 10);
    config.incoming = channels == 2;
    let target = config.output_target_fill_frames;
    SimulatedRoute::with_config(
        source,
        sink,
        reducer,
        input_rate as usize / 5,
        output_rate as usize / 5,
        target,
        config,
    )
    .unwrap()
}

#[test]
#[ignore = "four-minute drift scenarios: run explicitly with --release"]
fn production_resamplers_and_rings_correct_independent_clocks() {
    for ppm in [-750, -100, 100, 750] {
        let mut route = route(48_000, 48_000, 1, ppm, false);
        route.start().unwrap();
        let mut correction = 0i64;
        for second in 0..240 {
            let report = route.run_for_ns(1_000_000_000).unwrap();
            assert_eq!(report.input_ring.dropped_frames, 0);
            assert_eq!(report.output_ring.dropped_frames, 0);
            assert_eq!(report.output_ring.underrun_frames, 0);
            assert_eq!(route.metrics().model_resets, 0);
            assert!(route.metrics().estimated_latency_ms < 100.);
            assert!(route.metrics().drift_correction_ppm.abs() <= 1000);
            if second >= 180 {
                correction += i64::from(route.metrics().drift_correction_ppm);
            }
        }
        let average = correction / 60;
        eprintln!(
            "source={ppm} correction={average} fill={}",
            route.report().output_ring.fill_frames
        );
        assert!(
            (average + i64::from(ppm)).abs() < 60,
            "correction must compensate for the actual source clock: {ppm}, {average}"
        );
    }
}

#[test]
#[ignore = "five-minute clock-step scenario: run explicitly with --release"]
fn clock_rate_step_keeps_audio_continuous() {
    let mut route = route(44_100, 96_000, 2, 100, false);
    route.start().unwrap();
    route.run_for_ns(120_000_000_000).unwrap();
    route.set_source_drift_ppm(-100).unwrap();
    let mut average = 0i64;
    for second in 0..180 {
        route.run_for_ns(1_000_000_000).unwrap();
        if second >= 120 {
            average += i64::from(route.metrics().drift_correction_ppm);
        }
    }
    let report = route.report();
    assert_eq!(report.input_ring.dropped_frames, 0);
    assert_eq!(report.output_ring.dropped_frames, 0);
    assert_eq!(report.output_ring.underrun_frames, 0);
    assert_eq!(route.metrics().model_resets, 0);
    assert!(
        (average / 60 - 100).abs() < 100,
        "step correction={}",
        average / 60
    );
    assert!(route.metrics().estimated_latency_ms < 100.);
}

#[test]
fn rate_channel_and_irregular_callback_matrix_uses_the_production_graph() {
    for input in [44_100, 48_000, 96_000] {
        for output in [44_100, 48_000, 96_000] {
            for channels in [1, 2] {
                let mut route = route(input, output, channels, 100, true);
                route.start().unwrap();
                let startup = route.run_for_ns(2_000_000_000).unwrap();
                let report = route.run_for_ns(2_000_000_000).unwrap();
                assert_eq!(report.input_ring.dropped_frames, 0);
                assert_eq!(report.output_ring.dropped_frames, 0);
                assert_eq!(
                    report.output_ring.underrun_frames, startup.output_ring.underrun_frames,
                    "{input}->{output}, channels={channels}"
                );
                assert_eq!(report.reducer_failures, 0);
                assert_eq!(route.metrics().model_resets, 0);
                assert!(
                    route
                        .sink()
                        .captured_samples()
                        .iter()
                        .all(|s| s.is_finite() && s.abs() <= 0.98)
                );
                assert!(route.metrics().estimated_latency_ms < 100.);
            }
        }
    }
}

#[test]
fn injected_processing_cost_degrades_and_recovers_without_sleeping() {
    let mut route = route(48_000, 48_000, 2, 0, false);
    route.start().unwrap();
    route.set_processing_cost(Duration::from_millis(11));
    route.run_for_ns(100_000_000).unwrap();
    assert!(route.metrics().degraded);
    assert!(route.metrics().missed_deadlines >= 8);
    route.set_processing_cost(Duration::from_micros(500));
    route.run_for_ns(2_000_000_000).unwrap();
    assert!(!route.metrics().degraded);
    assert_eq!(route.metrics().model_resets, 1);
    assert!(route.metrics().skipped_inferences > 0);
    assert_eq!(route.report().output_ring.underrun_frames, 0);
}
