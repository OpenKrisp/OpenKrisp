use super::*;
use crate::bounded_audio_ring;
use audio_protocol::{ChannelCount, SampleFormat, SampleRateHz};
use denoise_dfn::{DfnNoiseReducer, default_model_path};
#[path = "../../../../tests/support/speech.rs"]
mod speech;

fn process(
    near: &[f32],
    far: &[f32],
    echo: bool,
    reference_enabled: bool,
) -> (Vec<f32>, crate::AecSnapshot) {
    let format = AudioFormat {
        sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
        channels: ChannelCount::new(1).unwrap(),
        sample_format: SampleFormat::F32,
        buffer_size_frames: None,
    };
    let (_tx, input) = bounded_audio_ring(4800, format.channels).unwrap();
    let (mut output, mut sink) = bounded_audio_ring(9600, format.channels).unwrap();
    let metrics = RouteMetricsStore::new(
        input.metrics(),
        output.metrics(),
        format.clone(),
        format.clone(),
    );
    let mut config = PipelineWorkerConfig::new(format.clone(), format, 3840);
    config.aec = Some(crate::AecDiagnostics::default());
    let mut reducer =
        DfnNoiseReducer::load(default_model_path(), DenoiseSettings::default()).unwrap();
    let mut graph = PipelineGraph::new(&config, reducer.descriptor(), &metrics, 9600).unwrap();
    let (aec, _capture, _render, _returned) = crate::aec::tests::stage();
    graph.model.aec = Some(aec);
    graph.set_processing_cost(Duration::ZERO);
    let mut result = Vec::new();
    let mut discarded = [0.; 960];
    for (frame, clean) in near.as_chunks::<480>().0.iter().enumerate() {
        let mut render = [0.; 960];
        for (i, channels) in render.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let n = (frame + 1) * 480 + i;
            channels[0] = far[n % far.len()];
            channels[1] = 0.6 * far[(n + 173) % far.len()];
        }
        if !reference_enabled {
            render.fill(0.);
        }
        crate::aec::tests::tick_with_render(
            graph.model.aec.as_mut().unwrap(),
            frame as u64,
            &render,
        );
        let mut capture = [0.; 480];
        for (i, x) in capture.iter_mut().enumerate() {
            let n = frame * 480 + i;
            *x = clean[i]
                + if echo && n >= 4800 {
                    0.6 * far[(n - 4800) % far.len()]
                } else {
                    0.
                };
        }
        graph
            .model
            .process_frame(&mut reducer, &mut output, &metrics, &capture)
            .unwrap();
        result.extend_from_slice(&graph.model.mixed_model_frame);
        sink.read(&mut discarded).unwrap();
        assert!(discarded.iter().all(|x| x.is_finite() && x.abs() <= 0.98));
    }
    assert_eq!(metrics.model_errors(), 0);
    (result, graph.model.aec.as_ref().unwrap().snapshot())
}

#[test]
#[ignore = "offline production AEC plus DFN corpus gate: run explicitly with --release"]
fn real_speech_survives_the_production_aec_then_dfn_chain() {
    let far = speech::speech(1);
    let mut near = vec![0.; 8 * speech::RATE]; // echo-only adaptation
    near.extend(speech::speech(0));
    let (reference, _) = process(&near, &far, false, true);
    let (actual, aec) = process(&near, &far, true, true);
    assert!(
        aec.processed_frames > 500,
        "quality test must exercise native AEC"
    );
    assert_eq!(aec.native_errors, 0);
    // Paired production-chain reference includes the exact APM phase response and DFN delay.
    let score = speech::score(
        &reference[8 * speech::RATE..],
        &actual[8 * speech::RATE..],
        0,
    );
    eprintln!("AEC+DFN real double-talk preservation={score:?} diagnostics={aec:?}");
    assert!(speech::preserves_speech(&score));
    // A paired reference alone could hide equal attenuation in both chains. Check
    // absolute speech level too, independent of the APM filterbank phase response.
    let delay = crate::AEC_DELAY_SAMPLES + 480;
    let window = 9 * speech::RATE..near.len() - speech::RATE;
    let clean_energy: f64 = window.clone().map(|i| f64::from(near[i]).powi(2)).sum();
    for output in [&reference, &actual] {
        let energy: f64 = window
            .clone()
            .map(|i| f64::from(output[i + delay]).powi(2))
            .sum();
        let level = (energy / clean_energy).sqrt();
        eprintln!("AEC+DFN absolute speech RMS gain={level}");
        assert!((0.5..1.5).contains(&level));
    }
    let input_energy: f64 = (6 * speech::RATE..8 * speech::RATE)
        .map(|n| f64::from(0.6 * far[(n - 4800) % far.len()]).powi(2))
        .sum();
    let residual: f64 = actual[6 * speech::RATE..8 * speech::RATE]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum();
    let erle = 10. * (input_energy / residual.max(1e-20)).log10();
    eprintln!("AEC+DFN real echo-only ERLE={erle}");
    assert!(erle > 3.);
    assert!(!speech::preserves_speech(&speech::score(
        &reference[8 * speech::RATE..],
        &vec![0.; actual.len()],
        0
    )));
    // Revoke the render reference: the unchanged gate must reject uncancelled echo.
    let (uncancelled, _) = process(&near, &far, true, false);
    let residual: f64 = uncancelled[6 * speech::RATE..8 * speech::RATE]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum();
    assert!(10. * (input_energy / residual.max(1e-20)).log10() < 3.);
}
