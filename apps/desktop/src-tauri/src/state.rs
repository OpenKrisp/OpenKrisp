use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

use audio_engine::{AudioEngine, AudioEngineHandle, EngineCommand, EngineError};
use audio_protocol::EngineSnapshot;

use crate::{config_store::ConfigStore, profile_writer::ProfileWriter};

struct Lifecycle {
    engine: Option<AudioEngine>,
    event_bridge: Option<JoinHandle<()>>,
}

pub(crate) trait EngineControl: Send + Sync {
    fn try_send(&self, command: EngineCommand) -> Result<(), EngineError>;
    fn snapshot(&self) -> EngineSnapshot;
}

impl EngineControl for AudioEngineHandle {
    fn try_send(&self, command: EngineCommand) -> Result<(), EngineError> {
        AudioEngineHandle::try_send(self, command)
    }

    fn snapshot(&self) -> EngineSnapshot {
        AudioEngineHandle::snapshot(self)
    }
}

/// Tauri-owned control-plane state. No callback or PCM buffer is reachable from here.
pub struct AppState {
    pub(crate) engine: Arc<dyn EngineControl>,
    pub config_store: ConfigStore,
    pub(crate) profile_writer: ProfileWriter,
    pub(crate) shutdown_started: AtomicBool,
    pub(crate) shutdown_finished: AtomicBool,
    lifecycle: Mutex<Lifecycle>,
}

impl AppState {
    pub fn new(
        engine: AudioEngine,
        config_store: ConfigStore,
        event_bridge: JoinHandle<()>,
    ) -> Result<Self, std::io::Error> {
        let profile_writer = ProfileWriter::start(config_store.clone())?;
        let handle = Arc::new(engine.handle());
        Ok(Self {
            engine: handle,
            profile_writer,
            shutdown_started: AtomicBool::new(false),
            shutdown_finished: AtomicBool::new(false),
            config_store,
            lifecycle: Mutex::new(Lifecycle {
                engine: Some(engine),
                event_bridge: Some(event_bridge),
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(engine: Arc<dyn EngineControl>, config_store: ConfigStore) -> Self {
        Self {
            profile_writer: ProfileWriter::start(config_store.clone())
                .expect("profile writer starts"),
            shutdown_started: AtomicBool::new(false),
            shutdown_finished: AtomicBool::new(false),
            engine,
            config_store,
            lifecycle: Mutex::new(Lifecycle {
                engine: None,
                event_bridge: None,
            }),
        }
    }

    /// Idempotently stops active routes, joins the supervisor, then joins the event bridge.
    pub fn shutdown(&self) -> Result<(), EngineError> {
        self.profile_writer.close();
        let mut lifecycle = match self.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(poisoned) => poisoned.into_inner(),
        };
        let engine_result = match lifecycle.engine.as_mut() {
            Some(engine) => engine.shutdown(),
            None => Ok(()),
        };
        lifecycle.engine = None;
        let bridge_result = lifecycle.event_bridge.take().map_or(Ok(()), |bridge| {
            bridge.join().map_err(|_| EngineError::SupervisorTerminated)
        });
        let profile_result = self.profile_writer.shutdown();
        self.shutdown_finished.store(true, Ordering::Release);
        engine_result.and(bridge_result).and(profile_result)
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
