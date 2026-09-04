use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};

use audio_protocol::DeviceId;

/// Stable categories translated from CPAL's non-exhaustive error kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AudioIoErrorKind {
    DeviceBusy,
    DeviceChanged,
    DeviceNotAvailable,
    HostUnavailable,
    InvalidInput,
    PermissionDenied,
    RealtimeDenied,
    ResourceExhausted,
    StreamInvalidated,
    UnsupportedConfig,
    UnsupportedOperation,
    Xrun,
    Backend,
    Other,
}

impl From<cpal::ErrorKind> for AudioIoErrorKind {
    fn from(kind: cpal::ErrorKind) -> Self {
        match kind {
            cpal::ErrorKind::DeviceBusy => Self::DeviceBusy,
            cpal::ErrorKind::DeviceChanged => Self::DeviceChanged,
            cpal::ErrorKind::DeviceNotAvailable => Self::DeviceNotAvailable,
            cpal::ErrorKind::HostUnavailable => Self::HostUnavailable,
            cpal::ErrorKind::InvalidInput => Self::InvalidInput,
            cpal::ErrorKind::PermissionDenied => Self::PermissionDenied,
            cpal::ErrorKind::RealtimeDenied => Self::RealtimeDenied,
            cpal::ErrorKind::ResourceExhausted => Self::ResourceExhausted,
            cpal::ErrorKind::StreamInvalidated => Self::StreamInvalidated,
            cpal::ErrorKind::UnsupportedConfig => Self::UnsupportedConfig,
            cpal::ErrorKind::UnsupportedOperation => Self::UnsupportedOperation,
            cpal::ErrorKind::Xrun => Self::Xrun,
            cpal::ErrorKind::BackendError => Self::Backend,
            _ => Self::Other,
        }
    }
}

/// Failure outside the real-time CPAL callbacks.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AudioIoError {
    #[error("CPAL {operation} failed: {kind:?}")]
    Backend {
        operation: &'static str,
        kind: AudioIoErrorKind,
    },
    #[error("no default {direction} device is available")]
    NoDefaultDevice { direction: &'static str },
    #[error("audio device was not found: {0}")]
    DeviceNotFound(DeviceId),
    #[error("invalid CPAL device identifier: {0}")]
    InvalidDeviceId(DeviceId),
    #[error("device has no supported {direction} PCM format with one or two channels")]
    NoSupportedFormat { direction: &'static str },
    #[error("callback scratch-buffer size overflowed addressable memory")]
    CallbackBufferOverflow,
}

pub(crate) fn map_cpal_error(operation: &'static str, error: cpal::Error) -> AudioIoError {
    AudioIoError::Backend {
        operation,
        kind: error.kind().into(),
    }
}

/// Compact stream issue category safe to record from an error callback.
pub type StreamIssueKind = AudioIoErrorKind;

/// Lock-free counters written by CPAL error callbacks.
#[derive(Clone, Debug, Default)]
pub struct StreamIssueStore {
    inner: Arc<StreamIssueAtoms>,
}

#[derive(Debug, Default)]
struct StreamIssueAtoms {
    total: AtomicU64,
    xruns: AtomicU64,
    terminal: AtomicU64,
    callback_violations: AtomicU64,
    last_kind: AtomicU8,
}

impl StreamIssueStore {
    pub(crate) fn record_cpal(&self, error: cpal::Error) {
        self.record(error.kind().into());
    }

    /// Records an invariant violation in a stream callback as terminal invalid input.
    pub fn record_callback_violation(&self) {
        self.inner
            .callback_violations
            .fetch_add(1, Ordering::Relaxed);
        self.record(StreamIssueKind::InvalidInput);
    }

    /// Records a compact category without formatting or allocation.
    pub fn record(&self, kind: StreamIssueKind) {
        self.inner.total.fetch_add(1, Ordering::Relaxed);
        if kind == StreamIssueKind::Xrun {
            self.inner.xruns.fetch_add(1, Ordering::Relaxed);
        }
        if is_terminal(kind) {
            self.inner.terminal.fetch_add(1, Ordering::Relaxed);
        }
        self.inner
            .last_kind
            .store(encode_kind(kind), Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self) -> StreamIssueSnapshot {
        StreamIssueSnapshot {
            total: self.inner.total.load(Ordering::Relaxed),
            xruns: self.inner.xruns.load(Ordering::Relaxed),
            terminal: self.inner.terminal.load(Ordering::Relaxed),
            callback_violations: self.inner.callback_violations.load(Ordering::Relaxed),
            last_kind: decode_kind(self.inner.last_kind.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamIssueSnapshot {
    pub total: u64,
    pub xruns: u64,
    pub terminal: u64,
    pub callback_violations: u64,
    pub last_kind: Option<StreamIssueKind>,
}

fn is_terminal(kind: StreamIssueKind) -> bool {
    kind != StreamIssueKind::Xrun
}

fn encode_kind(kind: StreamIssueKind) -> u8 {
    match kind {
        StreamIssueKind::DeviceBusy => 1,
        StreamIssueKind::DeviceChanged => 2,
        StreamIssueKind::DeviceNotAvailable => 3,
        StreamIssueKind::HostUnavailable => 4,
        StreamIssueKind::InvalidInput => 5,
        StreamIssueKind::PermissionDenied => 6,
        StreamIssueKind::RealtimeDenied => 7,
        StreamIssueKind::ResourceExhausted => 8,
        StreamIssueKind::StreamInvalidated => 9,
        StreamIssueKind::UnsupportedConfig => 10,
        StreamIssueKind::UnsupportedOperation => 11,
        StreamIssueKind::Xrun => 12,
        StreamIssueKind::Backend => 13,
        StreamIssueKind::Other => 14,
    }
}

fn decode_kind(value: u8) -> Option<StreamIssueKind> {
    Some(match value {
        1 => StreamIssueKind::DeviceBusy,
        2 => StreamIssueKind::DeviceChanged,
        3 => StreamIssueKind::DeviceNotAvailable,
        4 => StreamIssueKind::HostUnavailable,
        5 => StreamIssueKind::InvalidInput,
        6 => StreamIssueKind::PermissionDenied,
        7 => StreamIssueKind::RealtimeDenied,
        8 => StreamIssueKind::ResourceExhausted,
        9 => StreamIssueKind::StreamInvalidated,
        10 => StreamIssueKind::UnsupportedConfig,
        11 => StreamIssueKind::UnsupportedOperation,
        12 => StreamIssueKind::Xrun,
        13 => StreamIssueKind::Backend,
        14 => StreamIssueKind::Other,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_store_preserves_compact_categories() {
        let store = StreamIssueStore::default();
        store.record(StreamIssueKind::Xrun);
        store.record(StreamIssueKind::DeviceNotAvailable);
        let snapshot = store.snapshot();
        assert_eq!(snapshot.total, 2);
        assert_eq!(snapshot.xruns, 1);
        assert_eq!(snapshot.terminal, 1);
        assert_eq!(
            snapshot.last_kind,
            Some(StreamIssueKind::DeviceNotAvailable)
        );

        store.record(StreamIssueKind::PermissionDenied);
        assert_eq!(store.snapshot().terminal, 2);
        store.record_callback_violation();
        let snapshot = store.snapshot();
        assert_eq!(snapshot.callback_violations, 1);
        assert_eq!(snapshot.terminal, 3);
    }
}
