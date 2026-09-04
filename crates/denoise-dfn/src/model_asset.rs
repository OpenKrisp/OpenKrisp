use std::{
    fs::File,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use audio_protocol::{ChannelCount, SampleRateHz};
use denoise_core::{DenoiseError, NoiseReducerDescriptor};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MODEL_FILE: &str = "DeepFilterNet3_ll_onnx.tar.gz";
pub const SUPPORTED_MANIFEST_SCHEMA: u32 = 1;
pub const PINNED_UPSTREAM_REVISION: &str = "978576aa8400552a4ce9730838c635aa30db5e61";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelManifest {
    schema_version: u32,
    models: Vec<ModelManifestEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelManifestEntry {
    id: String,
    file: String,
    sha256: String,
    upstream_revision: String,
    purpose: String,
    sample_rate_hz: u32,
    channels: u16,
    frame_samples_per_channel: usize,
    algorithmic_latency_samples: usize,
}

/// Hash-validated model asset and manifest metadata.
#[derive(Clone, Debug)]
pub struct ModelAsset {
    path: PathBuf,
    expected_sha256: String,
    upstream_revision: String,
    purpose: String,
    descriptor: NoiseReducerDescriptor,
}

impl ModelAsset {
    /// Loads the adjacent manifest, validates its pinned entry, and hashes the model bytes.
    pub fn load(model_path: impl AsRef<Path>) -> Result<Self, DenoiseError> {
        let model_path = model_path.as_ref();
        File::open(model_path).map_err(map_asset_io)?;
        let model_file_name = model_path
            .file_name()
            .ok_or_else(|| model_load_error("model path has no file name"))?;
        let model_directory = model_path
            .parent()
            .ok_or_else(|| model_load_error("model path has no parent directory"))?;
        let manifest_path = model_directory.join("manifest.json");
        let manifest_bytes = std::fs::read(&manifest_path).map_err(map_asset_io)?;
        let manifest: ModelManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|error| model_load_error(format!("invalid model manifest: {error}")))?;
        if manifest.schema_version != SUPPORTED_MANIFEST_SCHEMA {
            return Err(model_load_error(format!(
                "unsupported model manifest schema {}",
                manifest.schema_version
            )));
        }

        let entry = manifest
            .models
            .into_iter()
            .find(|candidate| Path::new(&candidate.file).as_os_str() == model_file_name)
            .ok_or_else(|| model_load_error("model has no matching manifest entry"))?;
        validate_entry(&entry)?;

        let actual_sha256 = hash_file(model_path)?;
        if !actual_sha256.eq_ignore_ascii_case(&entry.sha256) {
            return Err(DenoiseError::ModelHashMismatch);
        }

        let sample_rate_hz =
            SampleRateHz::new(entry.sample_rate_hz).map_err(|_| DenoiseError::InvalidDescriptor)?;
        let channels =
            ChannelCount::new(entry.channels).map_err(|_| DenoiseError::InvalidDescriptor)?;
        let descriptor = NoiseReducerDescriptor {
            model_id: entry.id,
            sample_rate_hz,
            channels,
            frame_samples_per_channel: entry.frame_samples_per_channel,
            algorithmic_latency_samples: entry.algorithmic_latency_samples,
        };
        descriptor.frame_samples()?;

        Ok(Self {
            path: model_path.to_path_buf(),
            expected_sha256: entry.sha256,
            upstream_revision: entry.upstream_revision,
            purpose: entry.purpose,
            descriptor,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn expected_sha256(&self) -> &str {
        &self.expected_sha256
    }

    #[must_use]
    pub fn upstream_revision(&self) -> &str {
        &self.upstream_revision
    }

    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    #[must_use]
    pub const fn descriptor(&self) -> &NoiseReducerDescriptor {
        &self.descriptor
    }
}

/// Returns the repository-packaged low-latency model path.
#[must_use]
pub fn default_model_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../models")
        .join(MODEL_FILE)
}

fn validate_entry(entry: &ModelManifestEntry) -> Result<(), DenoiseError> {
    if entry.upstream_revision != PINNED_UPSTREAM_REVISION {
        return Err(model_load_error("model upstream revision is not pinned"));
    }
    let mut components = Path::new(&entry.file).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(model_load_error(
            "model manifest file must be a single file name",
        ));
    }
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(model_load_error("model manifest SHA-256 is malformed"));
    }
    if entry.id.trim().is_empty() || entry.purpose.trim().is_empty() {
        return Err(model_load_error("model manifest metadata is incomplete"));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, DenoiseError> {
    let file = File::open(path).map_err(map_asset_io)?;
    hash_reader(file)
}

fn hash_reader(mut reader: impl Read) -> Result<String, DenoiseError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1_024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| model_load_error(format!("could not read model asset: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn map_asset_io(error: io::Error) -> DenoiseError {
    if error.kind() == io::ErrorKind::NotFound {
        DenoiseError::ModelAssetMissing
    } else {
        model_load_error(format!("could not read model asset metadata: {error}"))
    }
}

fn model_load_error(message: impl Into<String>) -> DenoiseError {
    DenoiseError::ModelLoadFailed(message.into())
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn packaged_manifest_and_hash_are_valid() {
        let asset = ModelAsset::load(default_model_path()).expect("packaged model is valid");
        assert_eq!(asset.descriptor().sample_rate_hz.get(), 48_000);
        assert_eq!(asset.descriptor().channels.get(), 1);
        assert_eq!(asset.descriptor().frame_samples_per_channel, 480);
        assert_eq!(asset.descriptor().algorithmic_latency_samples, 480);
        assert_eq!(asset.upstream_revision(), PINNED_UPSTREAM_REVISION);
    }

    #[test]
    fn streaming_sha256_matches_reference_vector() {
        let hash = hash_reader(Cursor::new(b"abc")).expect("hashing succeeds");
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn missing_asset_is_typed() {
        let missing = default_model_path().with_file_name("missing-model.tar.gz");
        assert!(matches!(
            ModelAsset::load(missing),
            Err(DenoiseError::ModelAssetMissing)
        ));
    }

    #[test]
    fn hash_mismatch_is_rejected() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "openkrisp-model-hash-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).expect("temporary model directory");
        let model_path = directory.join("fixture.tar.gz");
        std::fs::write(&model_path, b"tampered model").expect("temporary model asset");
        let manifest = format!(
            r#"{{
                "schemaVersion": 1,
                "models": [{{
                    "id": "fixture",
                    "file": "fixture.tar.gz",
                    "sha256": "{}",
                    "upstreamRevision": "{}",
                    "purpose": "test",
                    "sampleRateHz": 48000,
                    "channels": 1,
                    "frameSamplesPerChannel": 480,
                    "algorithmicLatencySamples": 480
                }}]
            }}"#,
            "0".repeat(64),
            PINNED_UPSTREAM_REVISION
        );
        std::fs::write(directory.join("manifest.json"), manifest)
            .expect("temporary model manifest");

        let result = ModelAsset::load(model_path);
        std::fs::remove_dir_all(&directory).expect("temporary model cleanup");
        assert!(matches!(result, Err(DenoiseError::ModelHashMismatch)));
    }
}
