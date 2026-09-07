use std::{
    sync::{
        Mutex,
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
};

use audio_engine::EngineError;
use tauri::async_runtime::{Receiver, Sender, channel};

use crate::{
    api_error::ApiError,
    config_store::{AudioProfileDto, ConfigStore, ConfigStoreError},
};

const WRITE_CAPACITY: usize = 8;

pub(crate) enum ProfileMutation {
    Save(AudioProfileDto),
    Delete(String),
    Select(String),
}

struct WriteRequest {
    mutation: ProfileMutation,
    reply: Sender<Result<(), ConfigStoreError>>,
}

struct Lifecycle {
    sender: Option<SyncSender<WriteRequest>>,
    join: Option<JoinHandle<()>>,
}

/// One bounded, serialized disk worker. Async IPC tasks only enqueue and await a single reply.
pub(crate) struct ProfileWriter {
    lifecycle: Mutex<Lifecycle>,
}

impl ProfileWriter {
    pub(crate) fn start(store: ConfigStore) -> Result<Self, std::io::Error> {
        Self::start_with(move |mutation| match mutation {
            ProfileMutation::Save(profile) => store.save_profile(profile),
            ProfileMutation::Delete(id) => store.delete_profile(&id),
            ProfileMutation::Select(id) => store.select_profile(&id),
        })
    }

    fn start_with(
        mut write: impl FnMut(ProfileMutation) -> Result<(), ConfigStoreError> + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        let (sender, receiver) = sync_channel::<WriteRequest>(WRITE_CAPACITY);
        let join = thread::Builder::new()
            .name("openkrisp-profiles".to_owned())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let result = write(request.mutation);
                    // This reply channel is empty with capacity one; abandoned callers do not cancel
                    // an accepted disk transaction or prevent the remaining writes from draining.
                    let _ = request.reply.try_send(result);
                }
            })?;
        Ok(Self {
            lifecycle: Mutex::new(Lifecycle {
                sender: Some(sender),
                join: Some(join),
            }),
        })
    }

    fn enqueue(
        &self,
        mutation: ProfileMutation,
    ) -> Result<Receiver<Result<(), ConfigStoreError>>, ApiError> {
        let (reply, receiver) = channel(1);
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let sender = lifecycle
            .sender
            .as_ref()
            .ok_or_else(|| ApiError::from(EngineError::ShutdownInProgress))?;
        sender
            .try_send(WriteRequest { mutation, reply })
            .map_err(|error| {
                ApiError::from(match error {
                    TrySendError::Full(_) => EngineError::CommandQueueFull,
                    TrySendError::Disconnected(_) => EngineError::CommandChannelDisconnected,
                })
            })?;
        Ok(receiver)
    }

    pub(crate) async fn submit(&self, mutation: ProfileMutation) -> Result<(), ApiError> {
        self.enqueue(mutation)?
            .recv()
            .await
            .ok_or_else(|| ApiError::internal("The profile writer stopped unexpectedly."))?
            .map_err(|error| ApiError(error.to_dto()))
    }

    pub(crate) fn close(&self) {
        self.lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sender = None;
    }

    /// Reject new writes, finish all accepted writes, and join the disk owner thread.
    pub(crate) fn shutdown(&self) -> Result<(), EngineError> {
        let join = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            lifecycle.sender = None;
            lifecycle.join.take()
        };
        join.map_or(Ok(()), |join| {
            join.join().map_err(|_| EngineError::SupervisorTerminated)
        })
    }
}

impl Drop for ProfileWriter {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_protocol::ErrorCode;
    use std::time::Duration;

    #[test]
    fn writes_are_bounded_serialized_and_drained_even_if_callers_abandon_replies() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("profiles.json");
        let store = ConfigStore::open(path.clone());
        let (entered, entered_rx) = sync_channel(1);
        let (release, release_rx) = sync_channel(1);
        let mut first = true;
        let writer = ProfileWriter::start_with(move |mutation| {
            if first {
                first = false;
                entered.send(()).expect("entered disk operation");
                release_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("release disk");
            }
            match mutation {
                ProfileMutation::Save(profile) => store.save_profile(profile),
                ProfileMutation::Select(id) => store.select_profile(&id),
                ProfileMutation::Delete(id) => store.delete_profile(&id),
            }
        })
        .expect("writer");
        drop(
            writer
                .enqueue(ProfileMutation::Save(AudioProfileDto {
                    id: "office".to_owned(),
                    ..AudioProfileDto::default()
                }))
                .expect("save enqueued"),
        );
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("disk pending");
        let mut replies = Vec::new();
        for _ in 0..WRITE_CAPACITY {
            replies.push(
                writer
                    .enqueue(ProfileMutation::Select("office".to_owned()))
                    .expect("bounded slot"),
            );
        }
        assert_eq!(
            writer
                .enqueue(ProfileMutation::Select("office".to_owned()))
                .expect_err("queue full")
                .0
                .code,
            ErrorCode::CommandQueueFull
        );
        release.send(()).expect("release");
        writer
            .shutdown()
            .expect("accepted writes drained and joined");
        for mut reply in replies {
            reply
                .blocking_recv()
                .expect("reply")
                .expect("save precedes selection");
        }
        assert_eq!(
            ConfigStore::open(path).snapshot().selected_profile_id,
            "office"
        );
        assert_eq!(
            writer
                .enqueue(ProfileMutation::Select("office".to_owned()))
                .expect_err("closed")
                .0
                .code,
            ErrorCode::ShutdownInProgress
        );
    }

    #[test]
    fn asynchronous_reply_reports_disk_errors_without_mutating_the_snapshot() {
        let directory = tempfile::tempdir().expect("directory");
        // A regular file cannot be the profile directory, so the real persistence call fails.
        let parent_file = directory.path().join("not-a-directory");
        std::fs::write(&parent_file, b"preserve").expect("file");
        let store = ConfigStore::open(parent_file.join("profiles.json"));
        let before = store.snapshot();
        let writer = ProfileWriter::start(store.clone()).expect("writer");
        let error = tauri::async_runtime::block_on(
            writer.submit(ProfileMutation::Save(AudioProfileDto::default())),
        )
        .expect_err("disk failure returned");
        assert_eq!(error.0.code, ErrorCode::InvalidConfiguration);
        assert_eq!(store.snapshot(), before);
        writer.shutdown().expect("joined");
    }
}
