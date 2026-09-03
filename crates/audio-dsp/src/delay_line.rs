use audio_protocol::ChannelCount;

use crate::{DspError, interleaved::ensure_aligned};

/// Fixed, preallocated interleaved delay used to align the dry path with model output.
#[derive(Debug)]
pub struct FixedDryDelay {
    channels: ChannelCount,
    delay_frames: usize,
    samples: Vec<f32>,
    cursor: usize,
}

impl FixedDryDelay {
    /// Allocates the complete delay line and initializes it with silence.
    pub fn new(channels: ChannelCount, delay_frames: usize) -> Result<Self, DspError> {
        let samples = delay_frames
            .checked_mul(channels.as_usize())
            .ok_or(DspError::DelaySizeOverflow)?;
        Ok(Self {
            channels,
            delay_frames,
            samples: vec![0.0; samples],
            cursor: 0,
        })
    }

    /// Processes an arbitrary complete-frame-aligned chunk into caller-owned storage.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<usize, DspError> {
        ensure_aligned(input.len(), self.channels)?;
        if output.len() < input.len() {
            return Err(DspError::OutputBufferTooSmall {
                required: input.len(),
                actual: output.len(),
            });
        }
        if self.samples.is_empty() {
            output[..input.len()].copy_from_slice(input);
        } else {
            for (source, destination) in input.iter().zip(&mut output[..input.len()]) {
                *destination = self.samples[self.cursor];
                self.samples[self.cursor] = *source;
                self.cursor += 1;
                if self.cursor == self.samples.len() {
                    self.cursor = 0;
                }
            }
        }
        Ok(input.len() / self.channels.as_usize())
    }

    /// Restores the initial silent history.
    pub fn reset(&mut self) {
        self.samples.fill(0.0);
        self.cursor = 0;
    }

    /// Returns the configured delay in audio frames.
    #[must_use]
    pub const fn delay_frames(&self) -> usize {
        self.delay_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_is_exact_across_arbitrary_chunks() {
        let channels = ChannelCount::new(2).expect("valid channels");
        let mut delay = FixedDryDelay::new(channels, 2).expect("valid delay");
        let mut first = [9.0; 6];
        delay
            .process(&[1.0, 10.0, 2.0, 20.0, 3.0, 30.0], &mut first)
            .expect("valid buffer");
        assert_eq!(first, [0.0, 0.0, 0.0, 0.0, 1.0, 10.0]);

        let mut second = [0.0; 2];
        delay
            .process(&[4.0, 40.0], &mut second)
            .expect("valid buffer");
        assert_eq!(second, [2.0, 20.0]);
    }
}
