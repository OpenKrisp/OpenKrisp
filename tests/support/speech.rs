//! Offline, paired-signal regression helpers. Metrics are not intelligibility claims.
#![allow(dead_code)]
pub const RATE: usize = 48_000;
pub const FRAME: usize = 480;

pub fn speech(index: usize) -> Vec<f32> {
    let bytes: &[u8] = match index {
        0 => include_bytes!("../fixtures/speech/2277-149896-0026.pcm"),
        1 => include_bytes!("../fixtures/speech/2035-147960-0013.pcm"),
        _ => panic!("unknown corpus clip"),
    };
    let pcm = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|x| f32::from(i16::from_le_bytes([x[0], x[1]])) / 32768.)
        .collect::<Vec<_>>();
    // A fixed linear 16 -> 48 kHz conversion is sufficient for this paired regression.
    // Both reference and mixtures receive the identical conversion.
    let rms = (pcm.iter().map(|x| x * x).sum::<f32>() / pcm.len() as f32).sqrt();
    let mut result = vec![0.; RATE]; // one second for temporal startup
    for (i, x) in pcm.iter().enumerate() {
        let next = pcm.get(i + 1).copied().unwrap_or(*x);
        for sub in 0..3 {
            result.push((x + (next - x) * sub as f32 / 3.) * 0.08 / rms);
        }
    }
    result.resize(result.len().div_ceil(FRAME) * FRAME + RATE, 0.);
    result
}

pub fn mixture(clean: &[f32], kind: &str) -> Vec<f32> {
    let other = speech(1);
    let mut seed = 0x1020_3040_u32;
    let mut low = 0.;
    clean
        .iter()
        .enumerate()
        .map(|(i, sample)| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let white = seed as i32 as f32 / i32::MAX as f32;
            low = 0.96 * low + 0.04 * white;
            let t = i as f32 / RATE as f32;
            let noise = match kind {
                "fan" => 0.04 * white + 0.025 * (t * 2. * std::f32::consts::PI * 120.).sin(),
                "keyboard" => 0.18 * white * (-((i % 7200) as f32) / 300.).exp(),
                "street" => {
                    0.15 * low + 0.025 * white + 0.035 * (t * 2. * std::f32::consts::PI * 65.).sin()
                }
                "babble" => 0.5 * other[(i + RATE / 3) % other.len()],
                _ => panic!("unknown mixture"),
            };
            sample + noise
        })
        .collect()
}

#[derive(Debug)]
pub struct Score {
    pub si_sdr_db: f64,
    pub gain: f64,
    pub correlation: f64,
}

/// Evaluate after a declared fixed processing delay. Silence and startup are excluded.
pub fn score(clean: &[f32], output: &[f32], delay: usize) -> Score {
    let (mut rr, mut oo, mut ro) = (0.0, 0.0, 0.0);
    for i in RATE..clean.len() - RATE {
        let r = f64::from(clean[i]);
        let o = f64::from(output[i + delay]);
        rr += r * r;
        oo += o * o;
        ro += r * o;
    }
    let gain = ro / rr;
    let projected = ro * ro / rr;
    let residual = (oo - projected).max(1e-20);
    Score {
        gain,
        correlation: ro / (rr * oo).sqrt().max(1e-20),
        si_sdr_db: 10. * (projected.max(1e-20) / residual).log10(),
    }
}

pub fn preserves_speech(score: &Score) -> bool {
    (0.5..1.5).contains(&score.gain) && score.correlation > 0.8 && score.si_sdr_db > 5.
}

pub fn improves_noise(before: &Score, after: &Score) -> bool {
    (0.4..1.6).contains(&after.gain) && after.si_sdr_db - before.si_sdr_db > 0.5
}
