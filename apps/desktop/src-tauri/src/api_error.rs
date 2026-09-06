use audio_engine::EngineError;
use audio_protocol::{EngineErrorDto, ErrorCode};
use serde::Serialize;

/// Stable error shape returned by every Tauri command.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct ApiError(pub EngineErrorDto);

impl ApiError {
    #[must_use]
    pub fn invalid_configuration(message: impl Into<String>) -> Self {
        Self(EngineErrorDto::new(
            ErrorCode::InvalidConfiguration,
            message,
        ))
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self(EngineErrorDto::new(ErrorCode::Internal, message))
    }
}

impl From<EngineError> for ApiError {
    fn from(error: EngineError) -> Self {
        Self(error.to_dto())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_is_the_documented_flat_error_shape() {
        let encoded = serde_json::to_value(ApiError::from(EngineError::CommandQueueFull))
            .expect("API error serializes");
        assert_eq!(encoded["code"], "command_queue_full");
        assert!(
            encoded["message"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        assert!(encoded.get("details").is_some());
    }
}
