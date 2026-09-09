use super::*;
use crate::bounded_audio_ring;
use audio_protocol::{ChannelCount, SampleFormat, SampleRateHz};
use denoise_core::{
    FakeFailureMode, FakeNoiseReducer, FakeNoiseReducerConfig, IndependentChannels,
    NoiseReductionStats,
};

fn format(rate: u32, channels: u16) -> AudioFormat {
    AudioFormat {
        sample_rate_hz: SampleRateHz::new(rate).expect("rate"),
        channels: ChannelCount::new(channels).expect("channels"),
        sample_format: SampleFormat::F32,
        buffer_size_frames: None,
    }
}

fn reducer(channels: u16) -> IndependentChannels<FakeNoiseReducer> {
    IndependentChannels::new(ChannelCount::new(channels).expect("channels"), || {
        FakeNoiseReducer::new(FakeNoiseReducerConfig {
            descriptor: NoiseReducerDescriptor {
                model_id: "incoming-fixture".to_owned(),
                sample_rate_hz: SampleRateHz::new(48_000).expect("rate"),
                channels: ChannelCount::new(1).expect("mono"),
                frame_samples_per_channel: 480,
                algorithmic_latency_samples: 0,
            },
            settings: DenoiseSettings::default(),
            enabled_gain: 0.5,
            processing_delay: Duration::ZERO,
            failure_mode: FakeFailureMode::Never,
            stats: NoiseReductionStats::default(),
        })
    })
    .expect("independent channels")
}

fn render(
    input: &[f32],
    channels: u16,
    input_rate: u32,
    output_rate: u32,
    bypassed: bool,
) -> Vec<f32> {
    let mut reducer = reducer(channels);
    let input_format = format(input_rate, channels);
    let output_format = format(output_rate, channels);
    let (_, source) = bounded_audio_ring(4_800, input_format.channels).expect("source");
    let (mut output, mut sink) = bounded_audio_ring(4_800, output_format.channels).expect("sink");
    let metrics = RouteMetricsStore::new(
        source.metrics(),
        output.metrics(),
        input_format.clone(),
        output_format.clone(),
    );
    let mut config = PipelineWorkerConfig::new(input_format, output_format, 4_800);
    config.incoming = true;
    config.initially_bypassed = bypassed;
    let mut graph = PipelineGraph::new(
        &config,
        reducer.descriptor(),
        &metrics,
        output.capacity_frames(),
    )
    .expect("graph");
    let mut rendered = Vec::with_capacity(input.len() * 4);
    let mut scratch = vec![0.0; 4_800 * channels as usize];
    let mut offset = 0;
    let mut random = 71u32;
    let allocations = allocation_counter::measure(|| {
        while offset < input.len() {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let frames =
                (1 + random as usize % 347).min(graph.input_scratch.len() / channels as usize);
            let samples = (frames * channels as usize).min(input.len() - offset);
            graph.input_scratch[..samples].copy_from_slice(&input[offset..offset + samples]);
            graph
                .process_input(&mut reducer, &mut output, samples, &metrics)
                .expect("process fixture");
            let read = sink.read(&mut scratch).expect("render");
            rendered.extend_from_slice(&scratch[..read.read_frames * channels as usize]);
            offset += samples;
        }
    });
    assert_eq!(
        allocations.count_total, 0,
        "incoming steady state allocates: {allocations:?}"
    );
    assert_eq!(metrics.snapshot().model_resets, 0);
    assert_eq!(metrics.model_errors(), 0);
    if bypassed {
        assert!(metrics.snapshot().skipped_inferences > 0);
        assert_eq!(metrics.snapshot().inference_last_us, 0);
    }
    let expected = input.len() / channels as usize * output_rate as usize / input_rate as usize;
    assert!(
        expected.abs_diff(rendered.len() / channels as usize) < output_rate as usize / 20,
        "bounded resampler/frame tail: {expected} vs {}",
        rendered.len() / channels as usize
    );
    rendered
}

#[test]
fn incoming_music_bypass_and_speech_preserve_stereo_order_duration_and_gain() {
    for (input_rate, output_rate) in [(48_000, 48_000), (44_100, 32_000)] {
        let left: Vec<f32> = (0..input_rate)
            .map(|n| 0.2 * (n as f32 * 0.017).sin())
            .collect();
        let right: Vec<f32> = (0..input_rate)
            .map(|n| 0.1 * (n as f32 * 0.039).cos())
            .collect();
        let stereo: Vec<f32> = left
            .iter()
            .zip(&right)
            .flat_map(|(l, r)| [*l, *r])
            .collect();
        for bypassed in [true, false] {
            let rendered = render(&stereo, 2, input_rate, output_rate, bypassed);
            let mono_left = render(&left, 1, input_rate, output_rate, bypassed);
            let mono_right = render(&right, 1, input_rate, output_rate, bypassed);
            assert_eq!(rendered.len(), mono_left.len() * 2);
            for ((frame, l), r) in rendered
                .as_chunks::<2>()
                .0
                .iter()
                .zip(mono_left)
                .zip(mono_right)
            {
                assert!(
                    (frame[0] - l).abs() < 1e-6 && (frame[1] - r).abs() < 1e-6,
                    "stereo channels mixed or reordered"
                );
            }
        }
        let dry = render(&stereo, 2, input_rate, output_rate, true);
        let wet = render(&stereo, 2, input_rate, output_rate, false);
        assert_eq!(dry.len(), wet.len());
        for (dry, wet) in dry.iter().zip(wet) {
            assert!((dry * 0.5 - wet).abs() < 1e-6);
        }
    }
}

#[test]
fn incoming_priority_skips_inference_and_resets_once_before_resuming() {
    let mut reducer = reducer(2);
    let (_, source) = bounded_audio_ring(4_800, format(48_000, 2).channels).expect("source");
    let (mut output, mut sink) =
        bounded_audio_ring(4_800, format(48_000, 2).channels).expect("output");
    let metrics = RouteMetricsStore::new(
        source.metrics(),
        output.metrics(),
        format(48_000, 2),
        format(48_000, 2),
    );
    let mut config = PipelineWorkerConfig::new(format(48_000, 2), format(48_000, 2), 4_800);
    config.incoming = true;
    config.priority_pressure.store(true, Ordering::Release);
    let mut graph =
        PipelineGraph::new(&config, reducer.descriptor(), &metrics, 4_800).expect("graph");
    let mut scratch = [0.0; 2_000];
    for _ in 0..20 {
        graph
            .model
            .process_frame(&mut reducer, &mut output, &metrics, &[0.1; 960])
            .expect("frame");
        sink.read(&mut scratch).expect("sink");
        graph.reset_after_discontinuity(&mut reducer, &metrics);
    }
    assert!(metrics.snapshot().degraded);
    assert_eq!(metrics.snapshot().inference_last_us, 0);
    assert_eq!(metrics.snapshot().skipped_inferences, 20);
    assert_eq!(
        metrics.snapshot().model_resets,
        0,
        "suspended discontinuities must not consume model CPU"
    );
    config.priority_pressure.store(false, Ordering::Release);
    for _ in 0..5 {
        graph
            .model
            .process_frame(&mut reducer, &mut output, &metrics, &[0.1; 960])
            .expect("resume");
        sink.read(&mut scratch).expect("sink");
    }
    assert!(!metrics.snapshot().degraded);
    assert_eq!(metrics.snapshot().model_resets, 1);
    assert_eq!(metrics.snapshot().skipped_inferences, 20);
    config.priority_pressure.store(true, Ordering::Release);
    graph
        .model
        .process_frame(&mut reducer, &mut output, &metrics, &[0.1; 960])
        .expect("suspend again");
    sink.read(&mut scratch).expect("sink");
    config.priority_pressure.store(false, Ordering::Release);
    graph.reset_after_discontinuity(&mut reducer, &metrics);
    graph
        .model
        .process_frame(&mut reducer, &mut output, &metrics, &[0.1; 960])
        .expect("resume after discontinuity");
    assert_eq!(
        metrics.snapshot().model_resets,
        2,
        "a discontinuity reset also satisfies the pending resume reset"
    );
}

#[test]
fn incoming_stereo_cannot_silently_downmix_to_mono() {
    let (_, input) = bounded_audio_ring(4800, format(48000, 2).channels).expect("input");
    let (output, _) = bounded_audio_ring(4800, format(48000, 1).channels).expect("output");
    let metrics = RouteMetricsStore::new(
        input.metrics(),
        output.metrics(),
        format(48000, 2),
        format(48000, 1),
    );
    let mut config = PipelineWorkerConfig::new(format(48000, 2), format(48000, 1), 4800);
    config.incoming = true;
    assert!(matches!(
        PipelineGraph::new(&config, reducer(2).descriptor(), &metrics, 4800),
        Err(WorkerError::UnsupportedModelChannels)
    ));
}
