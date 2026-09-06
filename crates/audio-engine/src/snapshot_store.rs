use std::sync::{Arc, RwLock};

use audio_protocol::EngineSnapshot;

/// Low-contention immutable snapshots shared between the engine and control-plane readers.
///
/// Audio callbacks never access this store. The supervisor replaces the value at a low fixed
/// cadence, while UI readers clone the latest point-in-time view.
#[derive(Clone, Debug, Default)]
pub struct EngineSnapshotStore {
    inner: Arc<RwLock<EngineSnapshot>>,
}

impl EngineSnapshotStore {
    #[must_use]
    pub fn new(snapshot: EngineSnapshot) -> Self {
        Self {
            inner: Arc::new(RwLock::new(snapshot)),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> EngineSnapshot {
        match self.inner.read() {
            Ok(snapshot) => snapshot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub(crate) fn update(&self, update: impl FnOnce(&mut EngineSnapshot)) {
        let mut snapshot = match self.inner.write() {
            Ok(snapshot) => snapshot,
            Err(poisoned) => poisoned.into_inner(),
        };
        update(&mut snapshot);
        snapshot.revision = snapshot.revision.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_receive_immutable_revisions() {
        let store = EngineSnapshotStore::default();
        let before = store.snapshot();
        store.update(|snapshot| snapshot.shutting_down = true);
        let after = store.snapshot();

        assert_eq!(before.revision, 0);
        assert!(!before.shutting_down);
        assert_eq!(after.revision, 1);
        assert!(after.shutting_down);
    }
}
