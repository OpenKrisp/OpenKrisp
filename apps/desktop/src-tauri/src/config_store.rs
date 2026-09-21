use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::dto::DestinationKind;
use audio_protocol::{DenoiseSettings, DeviceId, EngineErrorDto, ErrorCode, ProcessingMode};
use serde::{Deserialize, Serialize};

pub const SETTINGS_SCHEMA_VERSION: u16 = 1;
const MAX_FILE_BYTES: u64 = 32_768;
const MAX_DEVICE_ID_BYTES: usize = 1_024;

/// Preferences may be incomplete or refer to unplugged devices. Start validates availability.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutePreferences {
    pub input_id: String,
    pub output_id: String,
    pub destination_kind: DestinationKind,
    pub mode: ProcessingMode,
    pub target_latency_ms: u16,
    pub denoise: DenoiseSettings,
    pub echo_cancellation: bool,
}

impl RoutePreferences {
    fn microphone() -> Self {
        Self {
            input_id: String::new(),
            output_id: String::new(),
            destination_kind: DestinationKind::VirtualMicrophone,
            mode: ProcessingMode::SpeechDenoise,
            target_latency_ms: 40,
            denoise: DenoiseSettings {
                post_filter_enabled: false,
                ..DenoiseSettings::default()
            },
            echo_cancellation: true,
        }
    }

    fn validate(&self) -> Result<(), ConfigStoreError> {
        for id in [&self.input_id, &self.output_id] {
            if id.len() > MAX_DEVICE_ID_BYTES
                || (!id.is_empty() && DeviceId::new(id.as_str()).is_err())
            {
                return Err(ConfigStoreError::Invalid("invalid device preference"));
            }
        }
        if self.target_latency_ms == 0 || self.target_latency_ms > 1_000 {
            return Err(ConfigStoreError::Invalid(
                "latency must be between 1 and 1000 ms",
            ));
        }
        self.denoise
            .validate()
            .map_err(|_| ConfigStoreError::Invalid("invalid denoise settings"))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopSettings {
    pub microphone: RoutePreferences,
    pub incoming: RoutePreferences,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            microphone: RoutePreferences::microphone(),
            incoming: RoutePreferences {
                destination_kind: DestinationKind::PhysicalOutput,
                mode: ProcessingMode::AlignedBypass,
                echo_cancellation: false,
                ..RoutePreferences::microphone()
            },
        }
    }
}

impl DesktopSettings {
    pub(crate) fn validate(&self) -> Result<(), ConfigStoreError> {
        self.microphone.validate()?;
        self.incoming.validate()?;
        if self.incoming.destination_kind != DestinationKind::PhysicalOutput
            || self.incoming.echo_cancellation
        {
            return Err(ConfigStoreError::Invalid(
                "incoming settings require physical playback and no capture AEC",
            ));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredSettings {
    schema_version: u16,
    settings: DesktopSettings,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("settings I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported settings schema")]
    UnsupportedSchema,
    #[error("invalid settings: {0}")]
    Invalid(&'static str),
}

impl ConfigStoreError {
    pub(crate) fn to_dto(&self) -> EngineErrorDto {
        let message = match self {
            Self::Io(_) => "Settings could not be read or saved on this device.",
            Self::UnsupportedSchema => {
                "Saved settings use an unsupported version and were not loaded."
            }
            Self::Json(_) | Self::Invalid(_) => "Saved settings are invalid and were not loaded.",
        };
        EngineErrorDto::new(ErrorCode::InvalidConfiguration, message)
    }
}

/// Used only by setup and the settings disk worker, never by audio threads.
pub struct ConfigStore {
    path: PathBuf,
    pub(crate) initial: DesktopSettings,
    pub(crate) load_warning: Option<EngineErrorDto>,
}

impl ConfigStore {
    pub fn open(path: PathBuf) -> Self {
        let (initial, load_warning) = match load_settings(&path) {
            Ok(settings) => (settings, None),
            Err(error) => (DesktopSettings::default(), Some(error.to_dto())),
        };
        Self {
            path,
            initial,
            load_warning,
        }
    }

    pub(crate) fn persist(&self, settings: &DesktopSettings) -> Result<(), ConfigStoreError> {
        settings.validate()?;
        let parent = self
            .path
            .parent()
            .ok_or(ConfigStoreError::Invalid("settings path has no parent"))?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(
            &mut temporary,
            &StoredSettings {
                schema_version: SETTINGS_SCHEMA_VERSION,
                settings: settings.clone(),
            },
        )?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.path)
            .map_err(|error| ConfigStoreError::Io(error.error))?;
        Ok(())
    }
}

fn load_settings(path: &Path) -> Result<DesktopSettings, ConfigStoreError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DesktopSettings::default());
        }
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ConfigStoreError::Invalid("settings file is too large"));
    }
    let stored: StoredSettings = serde_json::from_slice(&bytes)?;
    if stored.schema_version != SETTINGS_SCHEMA_VERSION {
        return Err(ConfigStoreError::UnsupportedSchema);
    }
    stored.settings.validate()?;
    Ok(stored.settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_routes_round_trip_incomplete_and_missing_devices_and_explicit_aec_off() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = ConfigStore::open(path.clone());
        assert!(!path.exists());
        let mut settings = store.initial.clone();
        settings.microphone.input_id = "missing:mic".into();
        settings.microphone.echo_cancellation = false;
        settings.microphone.denoise.attenuation_limit_db = Some(12.5);
        settings.incoming.output_id = "missing:speakers".into();
        settings.incoming.target_latency_ms = 70;
        store.persist(&settings).unwrap();
        let reopened = ConfigStore::open(path);
        assert!(reopened.load_warning.is_none());
        assert_eq!(reopened.initial, settings);
    }

    #[test]
    fn invalid_preferences_never_replace_the_last_valid_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = ConfigStore::open(path.clone());
        store.persist(&store.initial).unwrap();
        let original = fs::read(&path).unwrap();
        for case in 0..6 {
            let mut invalid = store.initial.clone();
            match case {
                0 => invalid.microphone.target_latency_ms = 0,
                1 => invalid.incoming.target_latency_ms = 1001,
                2 => invalid.incoming.destination_kind = DestinationKind::VirtualMicrophone,
                3 => invalid.incoming.echo_cancellation = true,
                4 => invalid.microphone.input_id = "a".repeat(MAX_DEVICE_ID_BYTES + 1),
                _ => invalid.microphone.output_id = "bad\nid".into(),
            }
            assert!(store.persist(&invalid).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn unreadable_future_and_oversized_settings_are_preserved_and_profiles_are_not_read() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(
            directory.path().join("profiles.json"),
            b"unused old profiles",
        )
        .unwrap();
        assert!(ConfigStore::open(path.clone()).load_warning.is_none());
        let future = serde_json::to_vec(&StoredSettings {
            schema_version: 999,
            settings: DesktopSettings::default(),
        })
        .unwrap();
        for bytes in [
            b"invalid json".to_vec(),
            future,
            vec![b' '; MAX_FILE_BYTES as usize + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            let store = ConfigStore::open(path.clone());
            assert!(store.load_warning.is_some());
            assert_eq!(store.initial, DesktopSettings::default());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
