use std::error::Error;
use std::f32::consts::TAU;
use std::io;
use std::path::{Path, PathBuf};

use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use static_assertions::assert_not_impl_any;

const MODEL_FILE: &str = "DeepFilterNet3_ll_onnx.tar.gz";
const SUPPORTED_MANIFEST_SCHEMA: u32 = 1;
const PINNED_UPSTREAM_REVISION: &str = "978576aa8400552a4ce9730838c635aa30db5e61";

// This is a property of the pinned upstream revision, not an architectural requirement. Keeping
// the assertion here prevents a future dependency update from silently changing the verified
// thread-trait finding before the adapter is reviewed.
assert_not_impl_any!(DfTract: Send, Sync);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelManifest {
    schema_version: u32,
    models: Vec<ModelManifestEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelManifestEntry {
    id: String,
    file: String,
    sha256: String,
    upstream_revision: String,
    purpose: String,
    sample_rate_hz: usize,
    channels: usize,
    frame_samples_per_channel: usize,
    algorithmic_latency_samples: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let model_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let manifest_entry = load_manifest_entry(&model_path)?;
    verify_model_hash(&model_path, &manifest_entry.sha256)?;

    let params = DfParams::new(model_path.clone())?;
    let runtime = RuntimeParams::default_with_ch(manifest_entry.channels);
    let mut model = DfTract::new(params, &runtime)?;
    verify_metadata(&model, &manifest_entry)?;

    let mut input = Array2::<f32>::zeros((manifest_entry.channels, model.hop_size));
    let mut output = Array2::<f32>::zeros((manifest_entry.channels, model.hop_size));

    let silence_lsnr_db = model.process(input.view(), output.view_mut())?;
    ensure_finite("silence output", output.iter().copied())?;

    fill_deterministic_fixture(&mut input, model.sr);
    let fixture_lsnr_db = model.process(input.view(), output.view_mut())?;
    ensure_finite("fixture output", output.iter().copied())?;

    let output_peak = output
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    let algorithmic_latency_samples =
        model.fft_size - model.hop_size + model.lookahead * model.hop_size;
    let algorithmic_latency_ms = algorithmic_latency_samples as f64 * 1_000.0 / model.sr as f64;

    println!("DeepFilterNet offline inference spike passed");
    println!("  model: {}", model_path.display());
    println!("  model id: {}", manifest_entry.id);
    println!("  purpose: {}", manifest_entry.purpose);
    println!("  upstream revision: {}", manifest_entry.upstream_revision);
    println!("  sha256: {}", manifest_entry.sha256);
    println!(
        "  layout: planar [channels, samples] = [{}, {}]",
        model.ch, model.hop_size
    );
    println!("  sample rate: {} Hz", model.sr);
    println!(
        "  frame/hop: {} samples ({:.1} ms)",
        model.hop_size,
        model.hop_size as f64 * 1_000.0 / model.sr as f64
    );
    println!("  FFT window: {} samples", model.fft_size);
    println!("  lookahead: {} frames", model.lookahead);
    println!(
        "  algorithmic latency: {algorithmic_latency_samples} samples ({algorithmic_latency_ms:.1} ms)"
    );
    println!("  silence local SNR: {silence_lsnr_db:.2} dB");
    println!("  fixture local SNR: {fixture_lsnr_db:.2} dB");
    println!("  fixture output peak: {output_peak:.6}");

    Ok(())
}

fn default_model_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../models")
        .join(MODEL_FILE)
}

fn load_manifest_entry(model_path: &Path) -> Result<ModelManifestEntry, Box<dyn Error>> {
    let model_directory = model_path.parent().ok_or_else(|| {
        invalid_data(format!(
            "model path has no parent directory: {}",
            model_path.display()
        ))
    })?;
    let manifest_path = model_directory.join("manifest.json");
    let manifest: ModelManifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    if manifest.schema_version != SUPPORTED_MANIFEST_SCHEMA {
        return Err(invalid_data(format!(
            "unsupported model manifest schema {} in {}",
            manifest.schema_version,
            manifest_path.display()
        ))
        .into());
    }

    let model_file_name = model_path.file_name().ok_or_else(|| {
        invalid_data(format!(
            "model path has no file name: {}",
            model_path.display()
        ))
    })?;
    let entry = manifest
        .models
        .into_iter()
        .find(|candidate| Path::new(&candidate.file).as_os_str() == model_file_name)
        .ok_or_else(|| {
            invalid_data(format!(
                "{} has no entry for {}",
                manifest_path.display(),
                model_path.display()
            ))
        })?;
    if entry.upstream_revision != PINNED_UPSTREAM_REVISION {
        return Err(invalid_data(format!(
            "manifest revision mismatch: expected {PINNED_UPSTREAM_REVISION}, got {}",
            entry.upstream_revision
        ))
        .into());
    }
    Ok(entry)
}

fn verify_model_hash(model_path: &Path, expected_hash: &str) -> Result<(), Box<dyn Error>> {
    let model_bytes = std::fs::read(model_path)?;
    let actual_hash = format!("{:x}", Sha256::digest(model_bytes));
    if actual_hash != expected_hash {
        return Err(invalid_data(format!(
            "model hash mismatch for {}: expected {expected_hash}, got {actual_hash}",
            model_path.display()
        ))
        .into());
    }
    Ok(())
}

fn verify_metadata(model: &DfTract, manifest: &ModelManifestEntry) -> Result<(), Box<dyn Error>> {
    let actual_latency_samples = model.fft_size - model.hop_size + model.lookahead * model.hop_size;
    let actual = (model.sr, model.ch, model.hop_size, actual_latency_samples);
    let expected = (
        manifest.sample_rate_hz,
        manifest.channels,
        manifest.frame_samples_per_channel,
        manifest.algorithmic_latency_samples,
    );
    if actual != expected {
        return Err(invalid_data(format!(
            "unexpected model metadata: expected {expected:?}, got {actual:?}"
        ))
        .into());
    }
    Ok(())
}

fn fill_deterministic_fixture(input: &mut Array2<f32>, sample_rate_hz: usize) {
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

fn ensure_finite(label: &str, samples: impl Iterator<Item = f32>) -> Result<(), Box<dyn Error>> {
    if samples.into_iter().all(f32::is_finite) {
        Ok(())
    } else {
        Err(invalid_data(format!("{label} contains a non-finite sample")).into())
    }
}

fn invalid_data(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
