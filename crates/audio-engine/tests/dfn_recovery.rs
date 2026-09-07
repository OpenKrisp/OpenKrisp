//! Opt-in production-model smoke with synthetic callbacks; no microphone or output device opens.
use std::{
    thread,
    time::{Duration, Instant},
};

use audio_engine::{
    PipelineWorkerConfig, RouteMetricsStore, bounded_audio_ring, spawn_pipeline_worker,
};
use audio_protocol::{AudioFormat, ChannelCount, DenoiseSettings, SampleFormat, SampleRateHz};
use denoise_dfn::{DfnNoiseReducer, default_model_path};

#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "This opt-in performance test requires a release build."
)]
#[ignore = "wall-clock performance smoke: run explicitly with --release on an idle host"]
fn production_model_recovers_from_a_single_input_overflow() {
    assert!(
        !cfg!(debug_assertions),
        "this performance smoke requires --release"
    );
    let format = AudioFormat {
        sample_rate_hz: SampleRateHz::new(48_000).expect("rate"),
        channels: ChannelCount::new(1).expect("mono"),
        sample_format: SampleFormat::F32,
        buffer_size_frames: None,
    };
    let (mut input, consumer) = bounded_audio_ring(4_800, format.channels).expect("input ring");
    let (mut output, mut sink) = bounded_audio_ring(9_600, format.channels).expect("output ring");
    output.write(&[0.0; 1_920]).expect("prefill");
    let metrics = RouteMetricsStore::new(
        consumer.metrics(),
        output.metrics(),
        format.clone(),
        format.clone(),
    );
    let mut worker = spawn_pipeline_worker(
        PipelineWorkerConfig::new(format.clone(), format, 3_840),
        consumer,
        output,
        metrics.clone(),
        || {
            let mut reducer =
                DfnNoiseReducer::load(default_model_path(), DenoiseSettings::default())?;
            reducer.warm_up(3)?;
            Ok(reducer)
        },
    )
    .expect("worker ready");
    let samples: Vec<_> = (0..480).map(|i| (i as f32 * 0.1).sin() * 0.1).collect();
    let overflow = vec![0.1; 5_280];
    let mut rendered = [0.0; 480];
    let started = Instant::now();
    let mut recovered = None;
    for tick in 0..400 {
        let deadline = started + Duration::from_millis(tick * 10);
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        if tick == 100 {
            input.write(&overflow).expect("inject one overflow");
        }
        input.write(&samples).expect("synthetic capture");
        worker.wake();
        sink.read_or_silence(&mut rendered)
            .expect("synthetic output");
        assert!(
            rendered
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 0.98)
        );
        if tick == 200 {
            recovered = Some(metrics.snapshot());
        }
    }
    worker.shutdown().expect("worker joined");
    let recovered = recovered.expect("recovery observation");
    let final_metrics = metrics.snapshot();
    eprintln!(
        "reset_last_us={} reset_total_us={} resets={} underruns={} recovered_underruns={} inference_p99_us={} degraded={}",
        final_metrics.reset_last_us,
        final_metrics.reset_total_us,
        final_metrics.model_resets,
        final_metrics.output_underruns,
        recovered.output_underruns,
        final_metrics.inference_p99_us,
        final_metrics.degraded
    );
    assert!(
        final_metrics.model_resets >= 1,
        "overflow must exercise the real reset path"
    );
    assert_eq!(
        final_metrics.model_resets, recovered.model_resets,
        "recovery must not self-trigger more resets"
    );
    assert!(
        final_metrics.output_underruns - recovered.output_underruns <= 2,
        "continuous output must resume after recovery"
    );
    assert!(
        !final_metrics.degraded,
        "healthy model inference must remain available"
    );
}
