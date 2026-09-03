/// Failure returned at a checked DSP boundary.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DspError {
    #[error("{samples} samples are not aligned to {channels} interleaved channels")]
    MisalignedInterleavedBuffer { samples: usize, channels: usize },
    #[error("output buffer needs {required} samples but has {actual}")]
    OutputBufferTooSmall { required: usize, actual: usize },
    #[error("unsupported channel mapping from {input_channels} to {output_channels} channels")]
    UnsupportedChannelMapping {
        input_channels: usize,
        output_channels: usize,
    },
    #[error("model frame size must be greater than zero")]
    ZeroModelFrameSize,
    #[error("model frame size overflowed addressable memory")]
    ModelFrameSizeOverflow,
    #[error("delay size overflowed addressable memory")]
    DelaySizeOverflow,
    #[error("crossfade length must be greater than zero frames")]
    ZeroCrossfadeFrames,
    #[error("limiter threshold must be finite and in the range (0, 1]")]
    InvalidLimiterThreshold,
    #[error("meter decay must be finite and in the range [0, 1]")]
    InvalidMeterDecay,
    #[error("resampler chunk size must be greater than zero frames")]
    ZeroResamplerChunkSize,
    #[error("resampler buffer size overflowed addressable memory")]
    ResamplerSizeOverflow,
    #[error("could not construct resampler: {0}")]
    ResamplerConstruction(String),
    #[error("resampling failed: {0}")]
    Resampling(String),
}
