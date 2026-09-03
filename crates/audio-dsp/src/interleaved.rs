use audio_protocol::ChannelCount;

use crate::DspError;

/// Semantic channel layout for the DSP policies implemented in Phase 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Discrete(ChannelCount),
}

impl ChannelLayout {
    /// Creates the canonical semantic layout for a channel count.
    #[must_use]
    pub fn from_channels(channels: ChannelCount) -> Self {
        match channels.get() {
            1 => Self::Mono,
            2 => Self::Stereo,
            _ => Self::Discrete(channels),
        }
    }

    /// Returns the number of channels represented by the layout.
    #[must_use]
    pub fn channels(self) -> ChannelCount {
        match self {
            Self::Mono => match ChannelCount::new(1) {
                Ok(channels) => channels,
                Err(_) => unreachable!(),
            },
            Self::Stereo => match ChannelCount::new(2) {
                Ok(channels) => channels,
                Err(_) => unreachable!(),
            },
            Self::Discrete(channels) => channels,
        }
    }
}

/// Checked borrowed view of interleaved `f32` PCM.
#[derive(Clone, Copy, Debug)]
pub struct InterleavedBlock<'a> {
    samples: &'a [f32],
    channels: ChannelCount,
}

impl<'a> InterleavedBlock<'a> {
    /// Validates that complete audio frames are present.
    pub fn new(samples: &'a [f32], channels: ChannelCount) -> Result<Self, DspError> {
        ensure_aligned(samples.len(), channels)?;
        Ok(Self { samples, channels })
    }

    /// Returns all interleaved samples.
    #[must_use]
    pub const fn as_slice(self) -> &'a [f32] {
        self.samples
    }

    /// Returns the channel count.
    #[must_use]
    pub const fn channels(self) -> ChannelCount {
        self.channels
    }

    /// Returns the number of complete audio frames.
    #[must_use]
    pub fn frames(self) -> usize {
        self.samples.len() / self.channels.as_usize()
    }

    /// Returns one complete interleaved audio frame.
    #[must_use]
    pub fn frame(self, index: usize) -> Option<&'a [f32]> {
        let channels = self.channels.as_usize();
        let start = index.checked_mul(channels)?;
        self.samples.get(start..start + channels)
    }
}

/// Checked mutable view of interleaved `f32` PCM.
#[derive(Debug)]
pub struct InterleavedBlockMut<'a> {
    samples: &'a mut [f32],
    channels: ChannelCount,
}

impl<'a> InterleavedBlockMut<'a> {
    /// Validates that complete audio frames are present.
    pub fn new(samples: &'a mut [f32], channels: ChannelCount) -> Result<Self, DspError> {
        ensure_aligned(samples.len(), channels)?;
        Ok(Self { samples, channels })
    }

    /// Returns all interleaved samples.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        self.samples
    }

    /// Returns the channel count.
    #[must_use]
    pub const fn channels(&self) -> ChannelCount {
        self.channels
    }

    /// Returns the number of complete audio frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.as_usize()
    }
}

pub(crate) fn ensure_aligned(samples: usize, channels: ChannelCount) -> Result<(), DspError> {
    let channels = channels.as_usize();
    if !samples.is_multiple_of(channels) {
        return Err(DspError::MisalignedInterleavedBuffer { samples, channels });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_view_reports_frames_and_channels() {
        let channels = ChannelCount::new(2).expect("valid channel count");
        let block =
            InterleavedBlock::new(&[1.0, 2.0, 3.0, 4.0], channels).expect("aligned test buffer");
        assert_eq!(block.frames(), 2);
        assert_eq!(block.frame(1), Some([3.0, 4.0].as_slice()));
        assert_eq!(block.frame(2), None);
        assert_eq!(
            ChannelLayout::from_channels(channels),
            ChannelLayout::Stereo
        );
    }

    #[test]
    fn checked_view_rejects_partial_frames() {
        let channels = ChannelCount::new(2).expect("valid channel count");
        assert!(matches!(
            InterleavedBlock::new(&[1.0, 2.0, 3.0], channels),
            Err(DspError::MisalignedInterleavedBuffer { .. })
        ));
    }
}
