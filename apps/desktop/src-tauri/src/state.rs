use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use audio_engine::{AudioEngine, AudioEngineHandle, EngineCommand, EngineError};
use audio_protocol::EngineSnapshot;

use crate::config_store::ConfigStore;

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
    lifecycle: Mutex<Lifecycle>,
}

impl AppState {
    #[must_use]
    pub fn new(
        engine: AudioEngine,
        config_store: ConfigStore,
        event_bridge: JoinHandle<()>,
    ) -> Self {
        let handle = Arc::new(engine.handle());
        Self {
            engine: handle,
            config_store,
            lifecycle: Mutex::new(Lifecycle {
                engine: Some(engine),
                event_bridge: Some(event_bridge),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(engine: Arc<dyn EngineControl>, config_store: ConfigStore) -> Self {
        Self {
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
        engine_result.and(bridge_result)
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
