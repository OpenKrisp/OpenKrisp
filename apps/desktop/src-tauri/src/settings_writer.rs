use std::{
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::Duration,
};

use audio_engine::EngineError;
use audio_protocol::EngineErrorDto;
use serde::{Deserialize, Serialize};

use crate::{
    api_error::ApiError,
    config_store::{ConfigStore, ConfigStoreError, DesktopSettings},
};

const COALESCE_WINDOW: Duration = Duration::from_millis(150);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub settings: DesktopSettings,
    /// Process-local revisions. Acceptance and durable save are deliberately separate.
    pub revision: u32,
    pub saved_revision: u32,
    pub load_warning: Option<EngineErrorDto>,
    pub write_error: Option<EngineErrorDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveSettingsRequest {
    pub settings: DesktopSettings,
    pub revision: u32,
    #[serde(default)]
    pub replace_unreadable: bool,
}

struct Shared {
    snapshot: SettingsSnapshot,
    pending: bool,
    closed: bool,
}

type Mailbox = Arc<(Mutex<Shared>, Condvar)>;

/// One latest document plus one in-flight disk write. No queue or waiter list can grow with edits.
/// Only control-plane threads touch this mailbox; disk I/O never holds its mutex.
pub struct SettingsWriter {
    shared: Mailbox,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl SettingsWriter {
    pub fn start(store: ConfigStore) -> Result<Self, std::io::Error> {
        let initial = store.initial.clone();
        let warning = store.load_warning.clone();
        Self::start_with(initial, warning, COALESCE_WINDOW, move |settings| {
            store.persist(settings)
        })
    }

    fn start_with(
        settings: DesktopSettings,
        load_warning: Option<EngineErrorDto>,
        window: Duration,
        write: impl FnMut(&DesktopSettings) -> Result<(), ConfigStoreError> + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        let shared = Arc::new((
            Mutex::new(Shared {
                snapshot: SettingsSnapshot {
                    settings,
                    revision: 0,
                    saved_revision: 0,
                    load_warning,
                    write_error: None,
                },
                pending: false,
                closed: false,
            }),
            Condvar::new(),
        ));
        let mailbox = Arc::clone(&shared);
        let join = thread::Builder::new()
            .name("openkrisp-settings".into())
            .spawn(move || run_writer(&mailbox, window, write))?;
        Ok(Self {
            shared,
            join: Mutex::new(Some(join)),
        })
    }

    pub fn snapshot(&self) -> SettingsSnapshot {
        lock(&self.shared.0).snapshot.clone()
    }

    pub fn submit(&self, request: SaveSettingsRequest) -> Result<SettingsSnapshot, ApiError> {
        request
            .settings
            .validate()
            .map_err(|error| ApiError(error.to_dto()))?;
        let mut shared = lock(&self.shared.0);
        if shared.closed {
            return Err(EngineError::ShutdownInProgress.into());
        }
        if request.revision <= shared.snapshot.revision {
            return Err(ApiError::invalid_configuration(
                "A newer settings edit was already received. Retry saving your latest choices.",
            ));
        }
        if shared.snapshot.load_warning.is_some() && !request.replace_unreadable {
            return Err(ApiError::invalid_configuration(
                "Saved settings could not be loaded. Choose Replace unreadable settings to enable saving.",
            ));
        }
        shared.snapshot.settings = request.settings;
        shared.snapshot.revision = request.revision;
        shared.snapshot.write_error = None;
        shared.pending = true;
        let snapshot = shared.snapshot.clone();
        self.shared.1.notify_one();
        Ok(snapshot)
    }

    pub fn close(&self) {
        lock(&self.shared.0).closed = true;
        self.shared.1.notify_one();
    }

    /// Stop accepting edits, skip the coalescing delay, and finish the latest accepted document.
    pub fn shutdown(&self) -> Result<(), EngineError> {
        self.close();
        if let Some(join) = lock(&self.join).take() {
            join.join().map_err(|_| EngineError::SupervisorTerminated)?;
        }
        let snapshot = self.snapshot();
        if snapshot.revision != snapshot.saved_revision {
            return Err(EngineError::InvalidConfiguration(
                "The latest settings could not be saved.",
            ));
        }
        Ok(())
    }
}

impl Drop for SettingsWriter {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn run_writer(
    mailbox: &Mailbox,
    window: Duration,
    mut write: impl FnMut(&DesktopSettings) -> Result<(), ConfigStoreError>,
) {
    loop {
        let mut shared = lock(&mailbox.0);
        while !shared.pending && !shared.closed {
            shared = mailbox
                .1
                .wait(shared)
                .unwrap_or_else(|error| error.into_inner());
        }
        if !shared.pending {
            return;
        }
        if !shared.closed {
            // Fixed window from the first edit; continuous editing cannot starve persistence.
            shared = mailbox
                .1
                .wait_timeout_while(shared, window, |state| !state.closed)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        let settings = shared.snapshot.settings.clone();
        let revision = shared.snapshot.revision;
        shared.pending = false;
        drop(shared);
        let result = write(&settings);
        let mut shared = lock(&mailbox.0);
        match result {
            Ok(()) => {
                shared.snapshot.saved_revision = revision;
                shared.snapshot.load_warning = None;
                if revision == shared.snapshot.revision {
                    shared.snapshot.write_error = None;
                }
            }
            Err(error) if revision == shared.snapshot.revision => {
                shared.snapshot.write_error = Some(error.to_dto());
            }
            Err(_) => {} // A newer accepted document will be attempted next.
        }
        mailbox.1.notify_all();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_protocol::ErrorCode;
    use std::sync::mpsc::sync_channel;

    fn request(revision: u32) -> SaveSettingsRequest {
        let mut settings = DesktopSettings::default();
        settings.microphone.target_latency_ms = u16::try_from(revision).unwrap();
        settings.incoming.output_id = format!("output:{revision}");
        SaveSettingsRequest {
            settings,
            revision,
            replace_unreadable: false,
        }
    }

    #[test]
    fn slow_disk_keeps_only_latest_pair_and_shutdown_drains_it_without_waiting_on_callers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = ConfigStore::open(path.clone());
        let (entered, entered_rx) = sync_channel(1);
        let (release, release_rx) = sync_channel(1);
        let writes = Arc::new(Mutex::new(Vec::new()));
        let written = Arc::clone(&writes);
        let writer = SettingsWriter::start_with(
            DesktopSettings::default(),
            None,
            Duration::ZERO,
            move |settings| {
                if lock(&written).is_empty() {
                    entered.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                lock(&written).push(settings.clone());
                store.persist(settings)
            },
        )
        .unwrap();
        writer.submit(request(1)).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for revision in 2..=500 {
            writer.submit(request(revision)).unwrap();
        }
        assert_eq!(writer.snapshot().saved_revision, 0);
        assert_eq!(writer.snapshot().revision, 500);
        // Reordered IPC cannot restore old preferences.
        assert!(writer.submit(request(499)).is_err());
        writer.close();
        assert_eq!(
            writer.submit(request(501)).unwrap_err().0.code,
            ErrorCode::ShutdownInProgress
        );
        release.send(()).unwrap();
        writer.shutdown().unwrap();
        assert_eq!(lock(&writes).len(), 2);
        assert_eq!(ConfigStore::open(path).initial, request(500).settings);
        assert_eq!(writer.snapshot().saved_revision, 500);
    }

    #[test]
    fn quit_flushes_an_edit_inside_the_coalescing_window() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let writer = SettingsWriter::start(ConfigStore::open(path.clone())).unwrap();
        writer.submit(request(65)).unwrap();
        writer.shutdown().unwrap();
        assert_eq!(ConfigStore::open(path).initial, request(65).settings);
        writer.shutdown().unwrap();
    }

    #[test]
    fn invalid_file_requires_explicit_replacement_and_invalid_edits_are_never_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(&path, b"preserve me").unwrap();
        let writer = SettingsWriter::start(ConfigStore::open(path.clone())).unwrap();
        assert!(writer.submit(request(1)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve me");
        let mut replace = request(1);
        replace.replace_unreadable = true;
        replace.settings.incoming.echo_cancellation = true;
        assert!(writer.submit(replace).is_err());
        assert_eq!(writer.snapshot().revision, 0);
        let mut replace = request(1);
        replace.replace_unreadable = true;
        writer.submit(replace).unwrap();
        writer.shutdown().unwrap();
        assert!(writer.snapshot().load_warning.is_none());
        assert!(ConfigStore::open(path).load_warning.is_none());
    }

    #[test]
    fn disk_errors_preserve_previous_file_and_latest_choices_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = ConfigStore::open(path.clone());
        store.persist(&DesktopSettings::default()).unwrap();
        let (finished, finished_rx) = sync_channel(2);
        let mut fail = true;
        let writer = SettingsWriter::start_with(
            DesktopSettings::default(),
            None,
            Duration::ZERO,
            move |settings| {
                let result = if fail {
                    fail = false;
                    Err(ConfigStoreError::Invalid("injected disk failure"))
                } else {
                    store.persist(settings)
                };
                finished.send(()).unwrap();
                result
            },
        )
        .unwrap();
        writer.submit(request(1)).unwrap();
        finished_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let state = writer
            .shared
            .1
            .wait_timeout_while(lock(&writer.shared.0), Duration::from_secs(5), |state| {
                state.snapshot.write_error.is_none()
            })
            .unwrap()
            .0;
        assert!(state.snapshot.write_error.is_some());
        assert_eq!(state.snapshot.settings, request(1).settings);
        assert_eq!(state.snapshot.saved_revision, 0);
        drop(state);
        assert_eq!(
            ConfigStore::open(path.clone()).initial,
            DesktopSettings::default()
        );
        writer.submit(request(2)).unwrap();
        writer.shutdown().unwrap();
        assert!(writer.snapshot().write_error.is_none());
        assert_eq!(writer.snapshot().saved_revision, 2);
        assert_eq!(ConfigStore::open(path).initial, request(2).settings);
    }
}
