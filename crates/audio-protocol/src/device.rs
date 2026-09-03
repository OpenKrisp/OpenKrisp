use std::num::{NonZeroU16, NonZeroU32};

use serde::{Deserialize, Serialize};

/// Device or canonical sample rate, explicitly measured in hertz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SampleRateHz(NonZeroU32);

impl SampleRateHz {
    /// Creates a non-zero sample rate.
    pub fn new(value: u32) -> Result<Self, UnitError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(UnitError::ZeroSampleRate)
    }

    /// Returns the rate in hertz.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<u32> for SampleRateHz {
    type Error = UnitError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Number of interleaved channels in one audio frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelCount(NonZeroU16);

impl ChannelCount {
    /// Creates a non-zero channel count.
    pub fn new(value: u16) -> Result<Self, UnitError> {
        NonZeroU16::new(value)
            .map(Self)
            .ok_or(UnitError::ZeroChannels)
    }

    /// Returns the channel count.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }

    /// Returns the channel count as `usize` for slice arithmetic.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.get() as usize
    }
}

impl TryFrom<u16> for ChannelCount {
    type Error = UnitError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Requested or negotiated device buffer size in audio frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BufferSizeFrames(NonZeroU32);

impl BufferSizeFrames {
    /// Creates a non-zero buffer size.
    pub fn new(value: u32) -> Result<Self, UnitError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(UnitError::ZeroBufferSize)
    }

    /// Returns the size in interleaved audio frames.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<u32> for BufferSizeFrames {
    type Error = UnitError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Stable, backend-neutral PCM sample representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SampleFormat {
    I8,
    I16,
    I24,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    DsdU8,
    DsdU16,
    DsdU32,
}

/// Complete negotiated PCM format for one endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFormat {
    pub sample_rate_hz: SampleRateHz,
    pub channels: ChannelCount,
    pub sample_format: SampleFormat,
    pub buffer_size_frames: Option<BufferSizeFrames>,
}

/// Invalid explicit-unit value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UnitError {
    #[error("sample rate must be greater than zero hertz")]
    ZeroSampleRate,
    #[error("channel count must be greater than zero")]
    ZeroChannels,
    #[error("buffer size must be greater than zero frames")]
    ZeroBufferSize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_units_reject_zero() {
        assert_eq!(SampleRateHz::new(0), Err(UnitError::ZeroSampleRate));
        assert_eq!(ChannelCount::new(0), Err(UnitError::ZeroChannels));
        assert_eq!(BufferSizeFrames::new(0), Err(UnitError::ZeroBufferSize));
    }
}
