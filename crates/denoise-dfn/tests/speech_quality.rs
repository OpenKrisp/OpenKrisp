#[path = "../../../tests/support/speech.rs"]
mod speech;
use audio_protocol::DenoiseSettings;
use denoise_core::NoiseReducer;
use denoise_dfn::{DfnNoiseReducer, default_model_path};

fn process(reducer: &mut DfnNoiseReducer, input: &[f32]) -> Vec<f32> {
    reducer.reset().unwrap();
    let mut output = vec![0.; input.len() + speech::RATE];
    for (frame, result) in input
        .as_chunks::<{ speech::FRAME }>()
        .0
        .iter()
        .zip(output.as_chunks_mut::<{ speech::FRAME }>().0.iter_mut())
    {
        reducer.process_planar(frame, result).unwrap();
    }
    assert!(output.iter().all(|x| x.is_finite() && x.abs() < 2.));
    output
}

#[test]
#[ignore = "offline corpus quality gate: run explicitly with --release"]
fn licensed_real_speech_is_preserved_and_noise_mixtures_improve() {
    let mut reducer =
        DfnNoiseReducer::load(default_model_path(), DenoiseSettings::default()).unwrap();
    let delay = reducer.descriptor().algorithmic_latency_samples;
    for clip in 0..2 {
        let clean = speech::speech(clip);
        let output = process(&mut reducer, &clean);
        let clean_score = speech::score(&clean, &output, delay);
        eprintln!("clip={clip} clean={clean_score:?} delay={delay}");
        assert!(speech::preserves_speech(&clean_score));
        let mut wrong_delay = output.clone();
        wrong_delay.rotate_left(speech::FRAME);
        assert!(
            !speech::preserves_speech(&speech::score(&clean, &wrong_delay, delay)),
            "gate must detect an incorrect frame delay"
        );
        if clip == 0 {
            let unrelated = speech::speech(1);
            let swapped: Vec<_> = (0..output.len())
                .map(|i| unrelated[i % unrelated.len()])
                .collect();
            assert!(
                !speech::preserves_speech(&speech::score(&clean, &swapped, delay)),
                "gate must detect swapped speech"
            );
        }
        for kind in ["fan", "keyboard", "street", "babble"] {
            let noisy = speech::mixture(&clean, kind);
            let output = process(&mut reducer, &noisy);
            let before = speech::score(&clean, &noisy, 0);
            let after = speech::score(&clean, &output, delay);
            let improvement = after.si_sdr_db - before.si_sdr_db;
            eprintln!(
                "clip={clip} {kind}: before={before:?} after={after:?} improvement={improvement}"
            );
            if kind == "babble" {
                // DFN does not separate speakers. Guard against damage, not speaker isolation.
                assert!(speech::preserves_speech(&after) && improvement > -1.);
            } else {
                assert!(
                    speech::improves_noise(&before, &after),
                    "noise removal regressed: {kind}"
                );
            }
            // Passing untouched noisy PCM or muting speech cannot satisfy the same gates.
            assert!(!speech::improves_noise(&before, &before));
            let muted = vec![0.; output.len()];
            assert!(!speech::preserves_speech(&speech::score(
                &clean, &muted, delay
            )));
        }
    }
    stereo_corpus_keeps_channels_independent(reducer.asset().clone());
}

fn stereo_corpus_keeps_channels_independent(asset: denoise_dfn::ModelAsset) {
    use audio_protocol::ChannelCount;
    use denoise_core::IndependentChannels;
    let left = speech::speech(0);
    let mut right = speech::speech(1);
    right.resize(left.len(), 0.);
    let noisy_left = speech::mixture(&left, "fan");
    let noisy_right = speech::mixture(&right, "street");
    let mut stereo = IndependentChannels::new(ChannelCount::new(2).unwrap(), || {
        DfnNoiseReducer::from_asset(asset.clone(), DenoiseSettings::default())
    })
    .unwrap();
    let mut references = [
        DfnNoiseReducer::from_asset(asset.clone(), DenoiseSettings::default()).unwrap(),
        DfnNoiseReducer::from_asset(asset, DenoiseSettings::default()).unwrap(),
    ];
    let delay = stereo.descriptor().algorithmic_latency_samples;
    let mut input = [0.; 960];
    let mut output = input;
    let mut reference = [0.; 480];
    let mut channels = [
        vec![0.; left.len() + speech::RATE],
        vec![0.; left.len() + speech::RATE],
    ];
    for frame in 0..left.len() / speech::FRAME {
        let start = frame * speech::FRAME;
        input[..480].copy_from_slice(&noisy_left[start..start + 480]);
        input[480..].copy_from_slice(&noisy_right[start..start + 480]);
        stereo.process_planar(&input, &mut output).unwrap();
        for channel in 0..2 {
            let offset = channel * speech::FRAME;
            references[channel]
                .process_planar(&input[offset..offset + 480], &mut reference)
                .unwrap();
            assert_eq!(
                &output[offset..offset + 480],
                reference,
                "stereo history or channel order changed"
            );
            channels[channel][start..start + 480].copy_from_slice(&reference);
        }
    }
    for (index, (clean, noisy)) in [(&left, &noisy_left), (&right, &noisy_right)]
        .into_iter()
        .enumerate()
    {
        let before = speech::score(clean, noisy, 0);
        let after = speech::score(clean, &channels[index], delay);
        eprintln!("stereo channel={index} before={before:?} after={after:?}");
        assert!(speech::improves_noise(&before, &after));
        assert!(!speech::preserves_speech(&speech::score(
            clean,
            &channels[1 - index],
            delay
        )));
    }
    stereo.reset().unwrap();
    input.fill(0.);
    stereo.process_planar(&input, &mut output).unwrap();
    assert!(
        output.iter().all(|x| x.abs() < 1e-7),
        "stereo reset retained speech history"
    );
}
