use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use audio_protocol::{
    DenoiseSettings, DeviceId, EngineErrorDto, ErrorCode, ProcessingMode, RouteId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dto::{DestinationKind, SourceKind};

pub const CONFIG_SCHEMA_VERSION: u16 = 3;
const DEFAULT_PREFERENCE: &str = "default";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioProfileDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source_kind: SourceKind,
    #[serde(default)]
    pub destination_kind: DestinationKind,
    #[serde(default = "default_profile_mode")]
    pub processing_mode: ProcessingMode,
    pub source_device_preference: String,
    pub destination_device_preference: String,
    pub target_latency_ms: u16,
    pub denoise: DenoiseSettings,
    #[serde(default = "audio_protocol::default_echo_cancellation")]
    pub echo_cancellation: bool,
}

fn default_profile_mode() -> ProcessingMode {
    ProcessingMode::SpeechDenoise
}

impl Default for AudioProfileDto {
    fn default() -> Self {
        Self {
            id: "default".to_owned(),
            name: "Default".to_owned(),
            source_kind: SourceKind::PhysicalInput,
            destination_kind: DestinationKind::PhysicalOutput,
            processing_mode: default_profile_mode(),
            source_device_preference: DEFAULT_PREFERENCE.to_owned(),
            destination_device_preference: DEFAULT_PREFERENCE.to_owned(),
            target_latency_ms: 40,
            denoise: DenoiseSettings::default(),
            echo_cancellation: true,
        }
    }
}

impl AudioProfileDto {
    fn validate(&self) -> Result<(), ConfigStoreError> {
        RouteId::new(&self.id)
            .map_err(|_| ConfigStoreError::InvalidProfile("invalid profile ID"))?;
        if self.id.len() > 64 {
            return Err(ConfigStoreError::InvalidProfile("profile ID is too long"));
        }
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || self.name.chars().any(char::is_control)
        {
            return Err(ConfigStoreError::InvalidProfile("invalid profile name"));
        }
        if self.source_kind == SourceKind::SystemLoopback
            && self.destination_kind != DestinationKind::PhysicalOutput
        {
            return Err(ConfigStoreError::InvalidProfile(
                "incoming profiles require a physical output",
            ));
        }
        if (self.source_kind == SourceKind::SystemLoopback
            && self.source_device_preference == DEFAULT_PREFERENCE)
            || (self.destination_kind == DestinationKind::VirtualMicrophone
                && self.destination_device_preference == DEFAULT_PREFERENCE)
        {
            return Err(ConfigStoreError::InvalidProfile(
                "virtual endpoints require an explicit preference",
            ));
        }
        validate_preference(&self.source_device_preference)?;
        validate_preference(&self.destination_device_preference)?;
        if self.target_latency_ms == 0 || self.target_latency_ms > 1_000 {
            return Err(ConfigStoreError::InvalidProfile(
                "target latency must be between 1 and 1000 milliseconds",
            ));
        }
        self.denoise
            .validate()
            .map_err(|_| ConfigStoreError::InvalidProfile("invalid denoise settings"))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCollectionDto {
    pub schema_version: u16,
    pub selected_profile_id: String,
    pub profiles: Vec<AudioProfileDto>,
    pub load_warning: Option<EngineErrorDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredConfig {
    schema_version: u16,
    selected_profile_id: String,
    profiles: Vec<AudioProfileDto>,
}

impl Default for StoredConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            selected_profile_id: "default".to_owned(),
            profiles: vec![AudioProfileDto::default()],
        }
    }
}

impl StoredConfig {
    fn validate(&self) -> Result<(), ConfigStoreError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigStoreError::UnsupportedSchema(self.schema_version));
        }
        if self.profiles.is_empty() {
            return Err(ConfigStoreError::InvalidProfile(
                "configuration must contain at least one profile",
            ));
        }
        for profile in &self.profiles {
            profile.validate()?;
        }
        for (index, profile) in self.profiles.iter().enumerate() {
            if self.profiles[..index]
                .iter()
                .any(|candidate| candidate.id == profile.id)
            {
                return Err(ConfigStoreError::InvalidProfile("duplicate profile ID"));
            }
        }
        if !self
            .profiles
            .iter()
            .any(|profile| profile.id == self.selected_profile_id)
        {
            return Err(ConfigStoreError::InvalidProfile(
                "selected profile does not exist",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("profile configuration I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("profile configuration JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported profile schema version {0}")]
    UnsupportedSchema(u16),
    #[error("invalid profile configuration: {0}")]
    InvalidProfile(&'static str),
    #[error("profile was not found: {0}")]
    ProfileNotFound(String),
}

impl ConfigStoreError {
    #[must_use]
    pub fn to_dto(&self) -> EngineErrorDto {
        let message = match self {
            Self::ProfileNotFound(_) => "The requested profile does not exist.",
            Self::UnsupportedSchema(_) => "Saved profiles use a newer schema and were not loaded.",
            Self::Io(_) => "Profiles could not be saved on this device.",
            Self::Json(_) | Self::InvalidProfile(_) => {
                "Saved profiles are invalid and were not loaded."
            }
        };
        EngineErrorDto::new(ErrorCode::InvalidConfiguration, message)
    }
}

/// Versioned profile persistence isolated from the audio engine and callback threads.
#[derive(Clone, Debug)]
pub struct ConfigStore {
    path: Arc<PathBuf>,
    config: Arc<RwLock<StoredConfig>>,
    mutation: Arc<Mutex<()>>,
    load_warning: Arc<RwLock<Option<EngineErrorDto>>>,
}

impl ConfigStore {
    /// Opens a config file, safely falling back to defaults without overwriting invalid input.
    #[must_use]
    pub fn open(path: PathBuf) -> Self {
        let loaded = load_config(&path);
        let (config, warning) = match loaded {
            Ok(config) => (config, None),
            Err(error) => (StoredConfig::default(), Some(error.to_dto())),
        };
        Self {
            path: Arc::new(path),
            config: Arc::new(RwLock::new(config)),
            mutation: Arc::new(Mutex::new(())),
            load_warning: Arc::new(RwLock::new(warning)),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> ProfileCollectionDto {
        let config = read_lock(&self.config);
        ProfileCollectionDto {
            schema_version: config.schema_version,
            selected_profile_id: config.selected_profile_id.clone(),
            profiles: config.profiles.clone(),
            load_warning: read_lock(&self.load_warning).clone(),
        }
    }

    pub fn save_profile(&self, profile: AudioProfileDto) -> Result<(), ConfigStoreError> {
        profile.validate()?;
        self.transact(|next| {
            match next
                .profiles
                .iter_mut()
                .find(|candidate| candidate.id == profile.id)
            {
                Some(existing) => *existing = profile,
                None => next.profiles.push(profile),
            }
            Ok(())
        })
    }

    pub fn delete_profile(&self, profile_id: &str) -> Result<(), ConfigStoreError> {
        self.transact(|next| {
            let before = next.profiles.len();
            next.profiles.retain(|profile| profile.id != profile_id);
            if next.profiles.len() == before {
                return Err(ConfigStoreError::ProfileNotFound(profile_id.to_owned()));
            }
            if next.profiles.is_empty() {
                next.profiles.push(AudioProfileDto::default());
            }
            if next.selected_profile_id == profile_id {
                next.selected_profile_id = next.profiles[0].id.clone();
            }
            Ok(())
        })
    }

    pub fn select_profile(&self, profile_id: &str) -> Result<(), ConfigStoreError> {
        self.transact(|next| {
            if !next.profiles.iter().any(|profile| profile.id == profile_id) {
                return Err(ConfigStoreError::ProfileNotFound(profile_id.to_owned()));
            }
            next.selected_profile_id = profile_id.to_owned();
            Ok(())
        })
    }

    fn transact(
        &self,
        change: impl FnOnce(&mut StoredConfig) -> Result<(), ConfigStoreError>,
    ) -> Result<(), ConfigStoreError> {
        self.transact_with(change, persist_config)
    }

    fn transact_with(
        &self,
        change: impl FnOnce(&mut StoredConfig) -> Result<(), ConfigStoreError>,
        persist: impl FnOnce(&Path, &StoredConfig) -> Result<(), ConfigStoreError>,
    ) -> Result<(), ConfigStoreError> {
        // Serialize read/modify/write transactions without holding the snapshot lock across I/O.
        let _mutation = self
            .mutation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut next = read_lock(&self.config).clone();
        change(&mut next)?;
        next.validate()?;
        persist(&self.path, &next)?;
        let mut current = write_lock(&self.config);
        *current = next;
        *write_lock(&self.load_warning) = None;
        Ok(())
    }
}

fn validate_preference(value: &str) -> Result<(), ConfigStoreError> {
    if value == DEFAULT_PREFERENCE {
        return Ok(());
    }
    DeviceId::new(value)
        .map(|_| ())
        .map_err(|_| ConfigStoreError::InvalidProfile("invalid device preference"))
}

fn load_config(path: &Path) -> Result<StoredConfig, ConfigStoreError> {
    if !path.exists() {
        return Ok(StoredConfig::default());
    }
    let bytes = fs::read(path)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    let schema = value.get("schemaVersion").and_then(Value::as_u64);
    let config = match schema {
        None | Some(0) => migrate_legacy(value)?,
        Some(1 | 2) => {
            let mut migrated: StoredConfig = serde_json::from_value(value)?;
            migrated.schema_version = CONFIG_SCHEMA_VERSION;
            migrated
        }
        Some(version) if version == u64::from(CONFIG_SCHEMA_VERSION) => {
            serde_json::from_value(value)?
        }
        Some(version) => {
            return Err(ConfigStoreError::UnsupportedSchema(
                u16::try_from(version).unwrap_or(u16::MAX),
            ));
        }
    };
    config.validate()?;
    Ok(config)
}

fn migrate_legacy(value: Value) -> Result<StoredConfig, ConfigStoreError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct LegacyConfig {
        #[serde(default)]
        schema_version: Option<u16>,
        selected_profile_id: String,
        profiles: Vec<AudioProfileDto>,
    }

    let legacy: LegacyConfig = serde_json::from_value(value)?;
    if legacy.schema_version.is_some_and(|version| version != 0) {
        return Err(ConfigStoreError::InvalidProfile(
            "invalid legacy schema version",
        ));
    }
    Ok(StoredConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        selected_profile_id: legacy.selected_profile_id,
        profiles: legacy.profiles,
    })
}

fn persist_config(path: &Path, config: &StoredConfig) -> Result<(), ConfigStoreError> {
    let parent = path.parent().ok_or(ConfigStoreError::InvalidProfile(
        "config path has no parent",
    ))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temporary, config)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| ConfigStoreError::Io(error.error))?;
    Ok(())
}

fn read_lock<T>(store: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    match store.read() {
        Ok(config) => config,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write_lock<T>(store: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    match store.write() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_two_enables_missing_aec_preference_and_preserves_explicit_off_after_reopen() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let mut legacy = serde_json::to_value(StoredConfig::default()).expect("JSON");
        legacy["schemaVersion"] = serde_json::json!(2);
        legacy["profiles"][0]
            .as_object_mut()
            .unwrap()
            .remove("echoCancellation");
        let original = serde_json::to_vec(&legacy).unwrap();
        fs::write(&path, &original).unwrap();
        let store = ConfigStore::open(path.clone());
        let loaded = store.snapshot();
        assert!(loaded.load_warning.is_none());
        assert_eq!(loaded.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(loaded.profiles[0].echo_cancellation);
        assert_eq!(
            fs::read(&path).unwrap(),
            original,
            "load does not rewrite existing work"
        );
        let mut disabled = loaded.profiles[0].clone();
        disabled.echo_cancellation = false;
        store.save_profile(disabled.clone()).unwrap();
        let reopened = ConfigStore::open(path).snapshot();
        assert!(reopened.load_warning.is_none());
        assert_eq!(reopened.profiles[0], disabled);
    }

    #[test]
    fn schema_one_migrates_physical_profiles_without_rewriting_the_file() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let original = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1, "selectedProfileId": "office",
            "profiles": [{ "id": "office", "name": "Office", "sourceDevicePreference": "missing:mic",
                "destinationDevicePreference": "default", "targetLatencyMs": 65,
                "denoise": {"enabled": false, "attenuationLimitDb": 18.5, "postFilterEnabled": false} }]
        })).expect("legacy JSON");
        fs::write(&path, &original).expect("legacy file");
        let store = ConfigStore::open(path.clone());
        let collection = store.snapshot();
        assert_eq!(collection.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(collection.load_warning.is_none());
        let profile = &collection.profiles[0];
        assert_eq!(profile.source_kind, SourceKind::PhysicalInput);
        assert_eq!(profile.destination_kind, DestinationKind::PhysicalOutput);
        assert_eq!(profile.processing_mode, ProcessingMode::SpeechDenoise);
        assert_eq!(profile.source_device_preference, "missing:mic");
        assert_eq!(profile.target_latency_ms, 65);
        assert!(!profile.denoise.enabled);
        assert_eq!(fs::read(&path).expect("unmodified file"), original);
        store
            .save_profile(profile.clone())
            .expect("explicit save migrates on disk");
        assert_eq!(ConfigStore::open(path).snapshot(), store.snapshot());
    }

    #[test]
    fn explicit_virtual_and_incoming_profiles_preserve_roles_and_mode_on_restart() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        let virtual_mic = AudioProfileDto {
            id: "virtual".to_owned(),
            destination_kind: DestinationKind::VirtualMicrophone,
            destination_device_preference: "macos:openkrisp-microphone".to_owned(),
            ..AudioProfileDto::default()
        };
        let incoming = AudioProfileDto {
            id: "incoming".to_owned(),
            source_kind: SourceKind::SystemLoopback,
            source_device_preference: "linux:pipewire-incoming".to_owned(),
            processing_mode: ProcessingMode::AlignedBypass,
            ..AudioProfileDto::default()
        };
        store
            .save_profile(virtual_mic.clone())
            .expect("save microphone");
        store.save_profile(incoming.clone()).expect("save incoming");
        store.select_profile("incoming").expect("select incoming");
        let reopened = ConfigStore::open(path).snapshot();
        assert_eq!(reopened.selected_profile_id, "incoming");
        assert!(reopened.profiles.contains(&virtual_mic));
        assert!(reopened.profiles.contains(&incoming));
    }

    #[test]
    fn invalid_profile_endpoint_roles_are_rejected_before_persistence() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        for profile in [
            AudioProfileDto {
                source_kind: SourceKind::SystemLoopback,
                ..AudioProfileDto::default()
            },
            AudioProfileDto {
                destination_kind: DestinationKind::VirtualMicrophone,
                ..AudioProfileDto::default()
            },
            AudioProfileDto {
                source_kind: SourceKind::SystemLoopback,
                source_device_preference: "loopback:source".to_owned(),
                destination_kind: DestinationKind::VirtualMicrophone,
                destination_device_preference: "virtual:output".to_owned(),
                ..AudioProfileDto::default()
            },
        ] {
            assert!(store.save_profile(profile).is_err());
            assert!(!path.exists());
        }
    }

    #[test]
    fn deleting_the_last_selected_profile_persists_a_valid_default() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        store
            .delete_profile("default")
            .expect("delete final profile");
        let reopened = ConfigStore::open(path).snapshot();
        assert_eq!(reopened.profiles, vec![AudioProfileDto::default()]);
        assert_eq!(reopened.selected_profile_id, "default");
        assert!(reopened.load_warning.is_none());
    }

    #[test]
    fn slow_or_failed_persistence_does_not_block_or_change_snapshots() {
        use std::{sync::mpsc::sync_channel, thread, time::Duration};
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        store
            .save_profile(AudioProfileDto::default())
            .expect("initial disk state");
        let before = store.snapshot();
        let bytes = fs::read(&path).expect("original bytes");
        let (entered, entered_rx) = sync_channel(1);
        let (release, release_rx) = sync_channel(1);
        let writer_store = store.clone();
        let writer = thread::spawn(move || {
            writer_store.transact_with(
                |next| {
                    next.profiles[0].name = "Uncommitted".to_owned();
                    Ok(())
                },
                |_, _| {
                    entered.send(()).expect("disk entered");
                    release_rx
                        .recv_timeout(Duration::from_secs(2))
                        .expect("release disk");
                    Err(ConfigStoreError::Io(std::io::Error::other(
                        "injected disk failure",
                    )))
                },
            )
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("write pending");
        assert_eq!(store.snapshot(), before);
        release.send(()).expect("release");
        assert!(writer.join().expect("joined").is_err());
        assert_eq!(store.snapshot(), before);
        assert_eq!(fs::read(path).expect("disk unchanged"), bytes);
    }

    #[test]
    fn concurrent_mutations_do_not_lose_updates() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        let mut writers = Vec::new();
        for index in 0..8 {
            let store = store.clone();
            writers.push(std::thread::spawn(move || {
                store.save_profile(AudioProfileDto {
                    id: format!("profile-{index}"),
                    ..AudioProfileDto::default()
                })
            }));
        }
        for writer in writers {
            writer.join().expect("joined").expect("saved");
        }
        assert_eq!(store.snapshot().profiles.len(), 9);
        assert_eq!(ConfigStore::open(path).snapshot(), store.snapshot());
    }

    #[test]
    fn profiles_persist_and_reopen_at_current_schema() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        let profile = AudioProfileDto {
            id: "airpods".to_owned(),
            name: "AirPods".to_owned(),
            source_device_preference: "coreaudio:input".to_owned(),
            destination_device_preference: "coreaudio:output".to_owned(),
            target_latency_ms: 60,
            ..AudioProfileDto::default()
        };
        store.save_profile(profile.clone()).expect("profile saves");
        store.select_profile("airpods").expect("profile selects");

        let reopened = ConfigStore::open(path).snapshot();
        assert_eq!(reopened.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(reopened.selected_profile_id, "airpods");
        assert!(reopened.profiles.contains(&profile));
        assert!(reopened.load_warning.is_none());
    }

    #[test]
    fn schema_zero_is_migrated_in_memory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("profiles.json");
        let legacy = serde_json::json!({
            "schemaVersion": 0,
            "selectedProfileId": "default",
            "profiles": [AudioProfileDto::default()]
        });
        fs::write(
            &path,
            serde_json::to_vec(&legacy).expect("legacy config serializes"),
        )
        .expect("legacy config writes");

        let snapshot = ConfigStore::open(path).snapshot();
        assert_eq!(snapshot.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(snapshot.load_warning.is_none());
    }

    #[test]
    fn schema_missing_legacy_config_is_also_migrated() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("profiles.json");
        let legacy = serde_json::json!({
            "selectedProfileId": "default",
            "profiles": [AudioProfileDto::default()]
        });
        fs::write(
            &path,
            serde_json::to_vec(&legacy).expect("legacy config serializes"),
        )
        .expect("legacy config writes");

        let snapshot = ConfigStore::open(path).snapshot();
        assert_eq!(snapshot.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(snapshot.load_warning.is_none());
    }

    #[test]
    fn corrupt_or_future_config_falls_back_without_overwriting_input() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("profiles.json");
        let original = br#"{"schemaVersion":999,"profiles":[]}"#;
        fs::write(&path, original).expect("future config writes");

        let snapshot = ConfigStore::open(path.clone()).snapshot();
        assert_eq!(snapshot.profiles, vec![AudioProfileDto::default()]);
        assert!(snapshot.load_warning.is_some());
        assert_eq!(fs::read(path).expect("input remains"), original);
    }

    #[test]
    fn invalid_profile_is_rejected_before_disk_mutation() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        let invalid = AudioProfileDto {
            target_latency_ms: 0,
            ..AudioProfileDto::default()
        };
        assert!(store.save_profile(invalid).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn successful_save_replaces_a_load_warning() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("profiles.json");
        fs::write(&path, b"not-json").expect("invalid config writes");
        let store = ConfigStore::open(path.clone());
        assert!(store.snapshot().load_warning.is_some());

        store
            .save_profile(AudioProfileDto::default())
            .expect("explicit save replaces invalid input");

        assert!(store.snapshot().load_warning.is_none());
        assert!(ConfigStore::open(path).snapshot().load_warning.is_none());
    }
}
