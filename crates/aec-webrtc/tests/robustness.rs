//! Additional echo-path controls for the prototype profile, without audio devices.
use aec_webrtc::{Aec3, FRAME_SAMPLES};

fn signal(samples: usize, mut seed: u32) -> Vec<f32> {
    let mut previous = 0.0;
    (0..samples)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            previous = 0.7 * previous + 0.3 * (seed as i32 as f32 / i32::MAX as f32);
            0.2 * previous
        })
        .collect()
}

#[test]
fn stereo_delay_steps_with_reflections_and_speaker_distortion() {
    const FRAMES: usize = 1800;
    let mut failures = Vec::new();
    for seed in [17, 743] {
        let left = signal(FRAMES * FRAME_SAMPLES, seed);
        let right = signal(FRAMES * FRAME_SAMPLES, seed + 91);
        for (left_gain, right_gain) in [
            (0.65, 0.3),
            (0.3, 0.65),
            (0.5, 0.5),
            (0.65, 0.0),
            (0.0, 0.65),
        ] {
            for nonlinear in [false, true] {
                let mut aec = Aec3::new(2).unwrap();
                let mut render = [[0.0; FRAME_SAMPLES]; 2];
                let mut input = [0.0; FRAME_SAMPLES];
                let mut output = input;
                // Windows: initial echo seconds 1–3, settled seconds 4–6,
                // then seconds 8–10 / 14–16 after the two delay changes.
                let mut energies = [(0.0f64, 0.0f64); 4];
                for frame in 0..FRAMES {
                    let phase = frame / 600;
                    let delay = [100 * 48, 160 * 48, 40 * 48][phase];
                    for i in 0..FRAME_SAMPLES {
                        let n = frame * FRAME_SAMPLES + i;
                        render[0][i] = left[n];
                        render[1][i] = right[n];
                        input[i] = 0.0;
                        // A small signed impulse response, applied to the speaker
                        // signal after optional smooth saturation. No fixture clips
                        // the microphone or passes values outside the native API.
                        for (extra_delay, gain) in [(0, 1.0), (240, 0.25), (528, -0.12)] {
                            if n >= delay + extra_delay + 31 {
                                let source = n - delay - extra_delay;
                                let distort = |v: f32| {
                                    if nonlinear {
                                        0.08 * (v / 0.08).tanh()
                                    } else {
                                        v
                                    }
                                };
                                input[i] += gain
                                    * (left_gain * distort(left[source])
                                        + right_gain * distort(right[source - 31]));
                            }
                        }
                    }
                    aec.analyze_render(&render).unwrap();
                    aec.process_capture(&input, &mut output).unwrap();
                    assert!(output.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
                    if (100..300).contains(&frame) {
                        energies[0].0 += input.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                        energies[0].1 += output.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                    }
                    let measured = if phase == 0 { 400..600 } else { 200..400 };
                    if measured.contains(&(frame % 600)) {
                        energies[phase + 1].0 +=
                            input.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                        energies[phase + 1].1 +=
                            output.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                    }
                }
                for (window, (input, output)) in energies.into_iter().enumerate() {
                    let erle = 10.0 * (input / output.max(1e-20)).log10();
                    println!(
                        "reflections seed={seed} gains={left_gain}/{right_gain} nonlinear={nonlinear} window={window} ERLE={erle:.3}"
                    );
                    if !erle.is_finite() || erle <= 10.0 {
                        failures.push((seed, left_gain, nonlinear, window, erle));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "echo recovery failed: {failures:?}");
}
