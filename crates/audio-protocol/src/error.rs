use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable machine-readable engine error code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidIdentifier,
    InvalidConfiguration,
    InvalidState,
    CommandQueueFull,
    DeviceNotFound,
    DeviceDisconnected,
    UnsupportedFormat,
    AudioIo,
    Dsp,
    Denoise,
    ModelAssetMissing,
    ModelHashMismatch,
    ModelLoadFailed,
    InvalidModelFrame,
    NonFiniteAudio,
    StreamError,
    Routing,
    WorkerTerminated,
    ShutdownInProgress,
    Internal,
}

impl ErrorCode {
    /// Returns the exact string emitted to UI clients.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidIdentifier => "invalid_identifier",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::InvalidState => "invalid_state",
            Self::CommandQueueFull => "command_queue_full",
            Self::DeviceNotFound => "device_not_found",
            Self::DeviceDisconnected => "device_disconnected",
            Self::UnsupportedFormat => "unsupported_format",
            Self::AudioIo => "audio_io",
            Self::Dsp => "dsp",
            Self::Denoise => "denoise",
            Self::ModelAssetMissing => "model_asset_missing",
            Self::ModelHashMismatch => "model_hash_mismatch",
            Self::ModelLoadFailed => "model_load_failed",
            Self::InvalidModelFrame => "invalid_model_frame",
            Self::NonFiniteAudio => "non_finite_audio",
            Self::StreamError => "stream_error",
            Self::Routing => "routing",
            Self::WorkerTerminated => "worker_terminated",
            Self::ShutdownInProgress => "shutdown_in_progress",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Sanitized error payload suitable for engine events and UI IPC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineErrorDto {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<String>,
    pub suggested_action: Option<String>,
}

impl EngineErrorDto {
    /// Creates a sanitized error without optional detail fields.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            suggested_action: None,
        }
    }

    /// Adds sanitized diagnostic details.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Adds a user-facing recovery suggestion.
    #[must_use]
    pub fn with_suggested_action(mut self, action: impl Into<String>) -> Self {
        self.suggested_action = Some(action.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_display_is_the_stable_wire_value() {
        assert_eq!(
            ErrorCode::CommandQueueFull.to_string(),
            "command_queue_full"
        );
    }
}
