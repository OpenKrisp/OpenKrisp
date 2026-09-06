use std::{sync::Mutex, thread::JoinHandle};

use audio_engine::{AudioEngine, AudioEngineHandle, EngineError};

use crate::config_store::ConfigStore;

struct Lifecycle {
    engine: Option<AudioEngine>,
    event_bridge: Option<JoinHandle<()>>,
}

/// Tauri-owned control-plane state. No callback or PCM buffer is reachable from here.
pub struct AppState {
    pub engine: AudioEngineHandle,
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
        let handle = engine.handle();
        Self {
            engine: handle,
            config_store,
            lifecycle: Mutex::new(Lifecycle {
                engine: Some(engine),
                event_bridge: Some(event_bridge),
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
