//! Deterministic synthetic speech-like quality gates. These do not establish intelligibility.
use aec_webrtc::{Aec3, FRAME_SAMPLES};

const RATE: usize = 48_000;
const FRAMES: usize = 1000;

fn speech(samples: usize, mut seed: u32, pitch: f64, period: f64) -> Vec<f32> {
    let mut noise = 0.0f32;
    (0..samples)
        .map(|i| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            noise = 0.7 * noise + 0.3 * (seed as i32 as f32 / i32::MAX as f32);
            let t = i as f64 / RATE as f64;
            let local = t % period;
            let phase = std::f64::consts::TAU * pitch * t + 0.5 * (t * 5.0).sin();
            match local / period {
                x if x < 0.15 => 0.2 * noise,
                x if x < 0.7 => {
                    let envelope = ((x - 0.15) * 30.0).min((0.7 - x) * 30.0).min(1.0);
                    (0.08
                        * envelope
                        * (1..=8)
                            .map(|h| (phase * f64::from(h)).sin() / f64::from(h))
                            .sum::<f64>()) as f32
                }
                x if x < 0.85 => 0.2 * noise,
                _ => 0.0,
            }
        })
        .collect()
}

#[derive(Default)]
struct Score {
    reference: f64,
    output: f64,
    projection: f64,
}
impl Score {
    fn add(&mut self, reference: f32, output: f32) {
        let r = f64::from(reference);
        let o = f64::from(output);
        self.reference += r * r;
        self.output += o * o;
        self.projection += r * o;
    }
    fn gain(&self) -> f64 {
        self.projection / self.reference
    }
    fn correlation(&self) -> f64 {
        self.projection / (self.reference * self.output).sqrt()
    }
}

#[test]
fn unrelated_render_preserves_local_speech() {
    // Playback that does not reach the microphone must not cause false
    // cancellation. Test startup as well as steady speech, including quieter
    // speech and render layouts that share harmonics but not an echo path.
    let far = speech(FRAMES * FRAME_SAMPLES, 739, 117.0, 0.43);
    let right = speech(FRAMES * FRAME_SAMPLES, 91, 237.0, 0.71);
    let mut failures = Vec::new();
    for seed in [541, 2053] {
        let near = speech(FRAMES * FRAME_SAMPLES, seed, 181.0, 0.6);
        for (channels, antiphase) in [(1, false), (2, false), (2, true)] {
            for scale in [0.5, 1.0] {
                let mut aec = Aec3::new(channels).unwrap();
                let mut baseline = Aec3::new(channels).unwrap();
                let mut render = [[0.0; FRAME_SAMPLES]; 2];
                let quiet = [[0.0; FRAME_SAMPLES]; 2];
                let mut input = [0.0; FRAME_SAMPLES];
                let mut output = input;
                let mut reference = input;
                let mut windows = [Score::default(), Score::default()];
                for frame in 0..FRAMES {
                    for i in 0..FRAME_SAMPLES {
                        let n = frame * FRAME_SAMPLES + i;
                        render[0][i] = far[n];
                        render[1][i] = if antiphase { -far[n] } else { right[n] };
                        input[i] = scale * near[n];
                    }
                    aec.analyze_render(&render[..channels]).unwrap();
                    aec.process_capture(&input, &mut output).unwrap();
                    baseline.analyze_render(&quiet[..channels]).unwrap();
                    baseline.process_capture(&input, &mut reference).unwrap();
                    let window = match frame {
                        100..300 => Some(0),
                        700..1000 => Some(1),
                        _ => None,
                    };
                    if let Some(window) = window {
                        for i in 0..FRAME_SAMPLES {
                            windows[window].add(reference[i], output[i]);
                        }
                    }
                }
                for (window, score) in windows.iter().enumerate() {
                    println!(
                        "no_echo seed={seed} channels={channels} antiphase={antiphase} scale={scale} window={window} gain={:.3} corr={:.3}",
                        score.gain(),
                        score.correlation()
                    );
                    if !(0.7..1.5).contains(&score.gain()) || score.correlation() <= 0.8 {
                        failures.push((seed, channels, antiphase, scale, window));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "local-only speech damaged: {failures:?}"
    );
}

#[test]
fn stronger_speech_like_double_talk_preserves_voiced_and_unvoiced_segments() {
    speech_like_matrix(&[2.0], 0, 0.0);
}

#[test]
fn varied_speech_like_double_talk_preserves_quiet_local_speech() {
    for (seed_shift, pitch_shift) in [(1000, 23.0), (2000, -31.0)] {
        speech_like_matrix(&[0.5, 1.0], seed_shift, pitch_shift);
    }
}

#[test]
fn speech_like_double_talk_acceptance() {
    speech_like_matrix(&[0.5, 1.0, 2.0], 0, 0.0);
}

fn speech_like_matrix(near_scales: &[f32], seed_shift: u32, pitch_shift: f64) {
    let near = speech(
        FRAMES * FRAME_SAMPLES,
        541 + seed_shift,
        181.0 - pitch_shift,
        0.6,
    );
    let far = speech(
        FRAMES * FRAME_SAMPLES,
        739 + seed_shift,
        117.0 + pitch_shift,
        0.43,
    );
    let right = speech(
        FRAMES * FRAME_SAMPLES,
        91 + seed_shift,
        237.0 + pitch_shift / 2.0,
        0.71,
    );
    let mut failures = Vec::new();
    for (channels, antiphase) in [(1, false), (2, false), (2, true)] {
        for &near_scale in near_scales {
            let mut aec = Aec3::new(channels).unwrap();
            let mut baseline = Aec3::new(channels).unwrap();
            let mut render = [[0.0; FRAME_SAMPLES]; 2];
            let quiet = [[0.0; FRAME_SAMPLES]; 2];
            let mut capture = [0.0; FRAME_SAMPLES];
            let mut near_input = [0.0; FRAME_SAMPLES];
            let mut output = [0.0; FRAME_SAMPLES];
            let mut near_output = [0.0; FRAME_SAMPLES];
            let (mut voiced, mut unvoiced) = (Score::default(), Score::default());
            let (mut echo_in, mut echo_out) = (0.0f64, 0.0f64);
            for frame in 0..FRAMES {
                for i in 0..FRAME_SAMPLES {
                    let n = frame * FRAME_SAMPLES + i;
                    render[0][i] = far[n];
                    render[1][i] = if antiphase { -far[n] } else { right[n] };
                    capture[i] = if n >= 4831 {
                        0.65 * far[n - 4800]
                            + if channels == 2 {
                                0.3 * if antiphase {
                                    -far[n - 4831]
                                } else {
                                    right[n - 4831]
                                }
                            } else {
                                0.0
                            }
                    } else {
                        0.0
                    };
                    near_input[i] = if frame >= 600 {
                        near_scale * near[n]
                    } else {
                        0.0
                    };
                    capture[i] += near_input[i];
                }
                aec.analyze_render(&render[..channels]).unwrap();
                aec.process_capture(&capture, &mut output).unwrap();
                baseline.analyze_render(&quiet[..channels]).unwrap();
                baseline
                    .process_capture(&near_input, &mut near_output)
                    .unwrap();
                assert!(output.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
                if (400..600).contains(&frame) {
                    echo_in += capture.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                    echo_out += output.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                }
                if frame >= 700 {
                    for i in 0..FRAME_SAMPLES {
                        // The peak delay selects interior phoneme windows only; paired APM output
                        // comparison, not this shift, accounts for the filterbank phase response.
                        let local =
                            ((frame * FRAME_SAMPLES + i - 430) as f64 / RATE as f64) % 0.6 / 0.6;
                        if (0.22..0.63).contains(&local) {
                            voiced.add(near_output[i], output[i]);
                        }
                        if (0.04..0.11).contains(&local) || (0.74..0.81).contains(&local) {
                            unvoiced.add(near_output[i], output[i]);
                        }
                    }
                }
            }
            let erle = 10.0 * (echo_in / echo_out.max(1e-20)).log10();
            println!(
                "seed_shift={seed_shift} pitch_shift={pitch_shift} channels={channels} antiphase={antiphase} near_scale={near_scale} ERLE={erle:.3} voiced_gain={:.3} voiced_corr={:.3} unvoiced_gain={:.3} unvoiced_corr={:.3}",
                voiced.gain(),
                voiced.correlation(),
                unvoiced.gain(),
                unvoiced.correlation()
            );
            if erle <= 10.0
                || !(0.7..1.5).contains(&voiced.gain())
                || voiced.correlation() <= 0.8
                || !(0.7..1.5).contains(&unvoiced.gain())
                || unvoiced.correlation() <= 0.8
            {
                failures.push((channels, antiphase, near_scale));
            }
        }
    }
    assert!(failures.is_empty(), "quality gates failed for {failures:?}");
}

#[test]
fn intermittent_stereo_reference_survives_pauses() {
    intermittent_reference(&[
        (1, false, false, false),
        (2, false, false, false),
        (2, true, false, false),
    ]);
}

#[test]
fn echo_gain_step_recovers_with_fixed_delay() {
    intermittent_reference(&[
        (1, false, false, true),
        (2, false, false, true),
        (2, true, false, true),
    ]);
}

#[test]
fn intermittent_echo_path_change_acceptance() {
    intermittent_reference(&[
        (1, false, true, true),
        (2, false, true, false),
        (2, false, true, true),
        (2, true, true, true),
    ]);
}

fn intermittent_reference(cases: &[(usize, bool, bool, bool)]) {
    let left = speech(FRAMES * FRAME_SAMPLES, 739, 117.0, 0.43);
    let right = speech(FRAMES * FRAME_SAMPLES, 91, 237.0, 0.71);
    let mut failures = Vec::new();
    for &(channels, antiphase, delay_change, gain_change) in cases {
        let mut aec = Aec3::new(channels).unwrap();
        let mut render = [[0.0; FRAME_SAMPLES]; 2];
        let mut input = [0.0; FRAME_SAMPLES];
        let mut output = [0.0; FRAME_SAMPLES];
        let mut energies = [(0.0f64, 0.0f64); 2];
        for frame in 0..FRAMES {
            for i in 0..FRAME_SAMPLES {
                let n = frame * FRAME_SAMPLES + i;
                render[0][i] = left[n];
                render[1][i] = if antiphase { -left[n] } else { right[n] };
                let delay = if delay_change && frame >= 600 {
                    7680
                } else {
                    4800
                };
                let gain = if gain_change && frame >= 600 {
                    0.35
                } else {
                    0.65
                };
                input[i] = if n >= delay + 31 {
                    gain * left[n - delay]
                        + if channels == 2 {
                            0.3 * if antiphase {
                                -left[n - delay - 31]
                            } else {
                                right[n - delay - 31]
                            }
                        } else {
                            0.0
                        }
                } else {
                    0.0
                };
            }
            aec.analyze_render(&render[..channels]).unwrap();
            aec.process_capture(&input, &mut output).unwrap();
            assert!(output.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
            let window = if (400..600).contains(&frame) {
                Some(0)
            } else if frame >= 800 {
                Some(1)
            } else {
                None
            };
            if let Some(window) = window {
                energies[window].0 += input.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                energies[window].1 += output.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
            }
        }
        for (window, (input, output)) in energies.into_iter().enumerate() {
            let erle = 10.0 * (input / output.max(1e-20)).log10();
            println!(
                "intermittent channels={channels} antiphase={antiphase} delay_change={delay_change} gain_change={gain_change} window={window} ERLE={erle:.3}"
            );
            if !erle.is_finite() || erle <= 10.0 {
                failures.push((channels, antiphase, delay_change, gain_change, window, erle));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "intermittent reference quality gates failed: {failures:?}"
    );
}
