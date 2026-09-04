use std::{error::Error, f32::consts::TAU, path::PathBuf};

use audio_protocol::DenoiseSettings;
use denoise_core::NoiseReducer;
use denoise_dfn::{DfnNoiseReducer, default_model_path};

fn main() -> Result<(), Box<dyn Error>> {
    let model_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let mut reducer = DfnNoiseReducer::load(&model_path, DenoiseSettings::default())?;
    let descriptor = reducer.descriptor().clone();
    let sample_count = descriptor.frame_samples()?;
    let mut input = vec![0.0; sample_count];
    let mut output = vec![0.0; sample_count];

    let silence_stats = reducer.process_planar(&input, &mut output)?;
    ensure_finite("silence output", &output)?;

    fill_deterministic_fixture(&mut input, descriptor.sample_rate_hz.get());
    let fixture_stats = reducer.process_planar(&input, &mut output)?;
    ensure_finite("fixture output", &output)?;
    let output_peak = output
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    let algorithmic_latency_ms = descriptor.algorithmic_latency_samples as f64 * 1_000.0
        / f64::from(descriptor.sample_rate_hz.get());

    println!("DeepFilterNet adapter offline inference passed");
    println!("  model: {}", model_path.display());
    println!("  model id: {}", descriptor.model_id);
    println!("  purpose: {}", reducer.asset().purpose());
    println!(
        "  upstream revision: {}",
        reducer.asset().upstream_revision()
    );
    println!("  sha256: {}", reducer.asset().expected_sha256());
    println!(
        "  layout: planar [channels, samples] = [{}, {}]",
        descriptor.channels.get(),
        descriptor.frame_samples_per_channel
    );
    println!("  sample rate: {} Hz", descriptor.sample_rate_hz.get());
    println!(
        "  frame/hop: {} samples ({:.1} ms)",
        descriptor.frame_samples_per_channel,
        descriptor.frame_samples_per_channel as f64 * 1_000.0
            / f64::from(descriptor.sample_rate_hz.get())
    );
    println!(
        "  algorithmic latency: {} samples ({algorithmic_latency_ms:.1} ms)",
        descriptor.algorithmic_latency_samples
    );
    println!(
        "  silence local SNR: {:.2} dB",
        silence_stats.local_snr_db.unwrap_or_default()
    );
    println!(
        "  fixture local SNR: {:.2} dB",
        fixture_stats.local_snr_db.unwrap_or_default()
    );
    println!("  fixture output peak: {output_peak:.6}");
    Ok(())
}

fn fill_deterministic_fixture(input: &mut [f32], sample_rate_hz: u32) {
    let mut noise_state = 0x5eed_1234_u32;
    for (sample_index, sample) in input.iter_mut().enumerate() {
        noise_state ^= noise_state << 13;
        noise_state ^= noise_state >> 17;
        noise_state ^= noise_state << 5;

        let time_seconds = sample_index as f32 / sample_rate_hz as f32;
        let speech_like =
            0.20 * (220.0 * TAU * time_seconds).sin() + 0.08 * (440.0 * TAU * time_seconds).sin();
        let normalized_noise = (noise_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
        *sample = speech_like + 0.04 * normalized_noise;
    }
}

fn ensure_finite(label: &str, samples: &[f32]) -> Result<(), Box<dyn Error>> {
    if samples.iter().all(|sample| sample.is_finite()) {
        Ok(())
    } else {
        Err(format!("{label} contains a non-finite sample").into())
    }
}
