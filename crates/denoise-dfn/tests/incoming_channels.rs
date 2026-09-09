use audio_protocol::{ChannelCount, DenoiseSettings};
use denoise_core::{IndependentChannels, NoiseReducer};
use denoise_dfn::{DfnNoiseReducer, ModelAsset, default_model_path};

#[test]
fn production_stereo_speech_fixture_keeps_independent_histories_and_resets() {
    std::thread::spawn(|| {
        let asset = ModelAsset::load(default_model_path()).expect("validated asset");
        let mut stereo = IndependentChannels::new(ChannelCount::new(2).expect("stereo"), || {
            DfnNoiseReducer::from_asset(asset.clone(), DenoiseSettings::default())
        })
        .expect("stereo reducer");
        let mut left = DfnNoiseReducer::from_asset(asset.clone(), DenoiseSettings::default())
            .expect("mono reference");
        let frames = stereo.descriptor().frame_samples_per_channel;
        let mut input = vec![0.0; frames * 2];
        let mut output = vec![0.0; frames * 2];
        let mut reference = vec![0.0; frames];
        let mut audible = false;
        for frame in 0..16 {
            for (n, sample) in input[..frames].iter_mut().enumerate() {
                let t = (frame * frames + n) as f32 / 48_000.;
                // Deterministic voiced harmonics, a syllabic envelope and low background noise.
                let envelope = (std::f32::consts::PI * t / 0.16).sin().max(0.0);
                *sample = envelope
                    * (0.18 * (t * 2. * std::f32::consts::PI * 180.).sin()
                        + 0.06 * (t * 2. * std::f32::consts::PI * 540.).sin())
                    + 0.005 * (t * 2. * std::f32::consts::PI * 3271.).sin();
            }
            stereo
                .process_planar(&input, &mut output)
                .expect("stereo speech");
            left.process_planar(&input[..frames], &mut reference)
                .expect("mono speech");
            assert_eq!(&output[..frames], reference);
            assert!(
                output[frames..].iter().all(|sample| sample.abs() < 1e-7),
                "left speech leaked into silent right history"
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
            audible |= output[..frames].iter().any(|sample| sample.abs() > 0.001);
        }
        assert!(
            audible,
            "fixture must exercise real inference and produce speech output"
        );
        stereo.reset().expect("reset both histories");
        input.fill(0.0);
        stereo
            .process_planar(&input, &mut output)
            .expect("clean reset");
        assert!(output.iter().all(|sample| sample.abs() < 1e-7));
    })
    .join()
    .expect("all model state stays on its constructing thread");
}
