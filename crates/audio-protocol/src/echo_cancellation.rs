use serde::{Deserialize, Serialize};

/// Processing availability, not an acoustic quality or measured latency claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum EchoCancellationStatus {
    WaitingReference,
    Aligning,
    Running,
    InvalidTiming,
    NativeError,
    Overloaded,
    Stopped,
}

/// Low-rate native AEC counters. No reference or microphone PCM crosses the control plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoCancellationSnapshot {
    pub status: EchoCancellationStatus,
    pub processed_frames: u64,
    pub fallback_frames: u64,
    pub resets: u64,
    pub native_errors: u64,
    pub max_processing_us: u64,
    pub missed_deadlines: u64,
}
