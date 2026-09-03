/// Backend-neutral denoising failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DenoiseError {
    #[error("model asset is missing")]
    ModelAssetMissing,
    #[error("model asset hash does not match its manifest")]
    ModelHashMismatch,
    #[error("model could not be loaded: {0}")]
    ModelLoadFailed(String),
    #[error("model format is unsupported")]
    UnsupportedModelFormat,
    #[error("invalid reducer descriptor")]
    InvalidDescriptor,
    #[error("invalid frame length: expected {expected}, received {actual}")]
    InvalidFrameLength { expected: usize, actual: usize },
    #[error("invalid channel count: expected {expected}, received {actual}")]
    InvalidChannelCount { expected: usize, actual: usize },
    #[error("invalid denoising setting: {0}")]
    InvalidSetting(String),
    #[error("input contains a non-finite sample")]
    NonFiniteInput,
    #[error("model output contains a non-finite sample")]
    NonFiniteOutput,
    #[error("inference failed: {0}")]
    InferenceFailed(String),
    #[error("model reset failed: {0}")]
    ResetFailed(String),
    #[error("setting change requires route restart")]
    RequiresRestart,
    #[error("fake reducer injected a failure on call {call}")]
    InjectedFailure { call: u64 },
}
