use audio_protocol::ChannelCount;

use crate::{ChannelLayout, DspError, interleaved::ensure_aligned};

/// Explicit mapping policy selected from source and destination layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelMappingPolicy {
    Passthrough,
    StereoToMonoAverage,
    MonoToStereoDuplicate,
}

/// Allocation-free mapper for the initially supported mono/stereo policies.
#[derive(Clone, Copy, Debug)]
pub struct ChannelMapper {
    input_channels: ChannelCount,
    output_channels: ChannelCount,
    policy: ChannelMappingPolicy,
}

impl ChannelMapper {
    /// Selects the documented mapping policy for two layouts.
    pub fn new(input: ChannelLayout, output: ChannelLayout) -> Result<Self, DspError> {
        let input_channels = input.channels();
        let output_channels = output.channels();
        let policy = match (input_channels.get(), output_channels.get()) {
            (input, output) if input == output => ChannelMappingPolicy::Passthrough,
            (2, 1) => ChannelMappingPolicy::StereoToMonoAverage,
            (1, 2) => ChannelMappingPolicy::MonoToStereoDuplicate,
            (input_channels, output_channels) => {
                return Err(DspError::UnsupportedChannelMapping {
                    input_channels: usize::from(input_channels),
                    output_channels: usize::from(output_channels),
                });
            }
        };
        Ok(Self {
            input_channels,
            output_channels,
            policy,
        })
    }

    /// Returns the fixed policy chosen at construction.
    #[must_use]
    pub const fn policy(self) -> ChannelMappingPolicy {
        self.policy
    }

    /// Maps complete interleaved frames into a caller-owned output buffer.
    pub fn process(&self, input: &[f32], output: &mut [f32]) -> Result<usize, DspError> {
        ensure_aligned(input.len(), self.input_channels)?;
        let frames = input.len() / self.input_channels.as_usize();
        let required = frames.checked_mul(self.output_channels.as_usize()).ok_or(
            DspError::OutputBufferTooSmall {
                required: usize::MAX,
                actual: output.len(),
            },
        )?;
        if output.len() < required {
            return Err(DspError::OutputBufferTooSmall {
                required,
                actual: output.len(),
            });
        }

        match self.policy {
            ChannelMappingPolicy::Passthrough => output[..required].copy_from_slice(input),
            ChannelMappingPolicy::StereoToMonoAverage => {
                let (input_frames, remainder) = input.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                for (destination, source) in output[..frames].iter_mut().zip(input_frames) {
                    *destination = (source[0] + source[1]) * 0.5;
                }
            }
            ChannelMappingPolicy::MonoToStereoDuplicate => {
                let (output_frames, remainder) = output[..required].as_chunks_mut::<2>();
                debug_assert!(remainder.is_empty());
                for (source, destination) in input.iter().zip(output_frames) {
                    destination.fill(*source);
                }
            }
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_and_stereo_policies_preserve_frame_order() {
        let downmix = ChannelMapper::new(ChannelLayout::Stereo, ChannelLayout::Mono)
            .expect("supported mapping");
        let mut mono = [0.0; 3];
        assert_eq!(
            downmix.process(&[1.0, -1.0, 0.25, 0.75, -0.5, -0.25], &mut mono),
            Ok(3)
        );
        assert_eq!(mono, [0.0, 0.5, -0.375]);

        let upmix = ChannelMapper::new(ChannelLayout::Mono, ChannelLayout::Stereo)
            .expect("supported mapping");
        let mut stereo = [0.0; 6];
        assert_eq!(upmix.process(&mono, &mut stereo), Ok(3));
        assert_eq!(stereo, [0.0, 0.0, 0.5, 0.5, -0.375, -0.375]);
    }

    #[test]
    fn matching_layout_is_passthrough() {
        let mapper = ChannelMapper::new(ChannelLayout::Stereo, ChannelLayout::Stereo)
            .expect("supported mapping");
        let input = [1.0, 2.0, 3.0, 4.0];
        let mut output = [0.0; 4];
        assert_eq!(mapper.process(&input, &mut output), Ok(2));
        assert_eq!(output, input);
    }

    #[test]
    fn unsupported_discrete_mapping_is_rejected() {
        let surround = ChannelCount::new(3).expect("valid channel count");
        assert_eq!(
            ChannelMapper::new(ChannelLayout::Discrete(surround), ChannelLayout::Mono)
                .expect_err("mapping should be rejected"),
            DspError::UnsupportedChannelMapping {
                input_channels: 3,
                output_channels: 1,
            }
        );
    }
}
