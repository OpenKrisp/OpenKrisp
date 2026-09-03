use std::f32::consts::FRAC_PI_2;

use audio_protocol::ChannelCount;

use crate::{DspError, interleaved::ensure_aligned};

/// Endpoint selected by the latency-aligned crossfader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossfadeTarget {
    Dry,
    Wet,
}

impl CrossfadeTarget {
    const fn wet_mix(self) -> f32 {
        match self {
            Self::Dry => 0.0,
            Self::Wet => 1.0,
        }
    }
}

/// Equal-power wet/dry crossfader with a fixed transition length.
#[derive(Debug)]
pub struct EqualPowerCrossfade {
    channels: ChannelCount,
    fade_frames: usize,
    current_mix: f32,
    start_mix: f32,
    target_mix: f32,
    transition_frame: usize,
    transitioning: bool,
}

impl EqualPowerCrossfade {
    /// Creates a crossfader at one exact endpoint.
    pub fn new(
        channels: ChannelCount,
        fade_frames: usize,
        initial: CrossfadeTarget,
    ) -> Result<Self, DspError> {
        if fade_frames == 0 {
            return Err(DspError::ZeroCrossfadeFrames);
        }
        let mix = initial.wet_mix();
        Ok(Self {
            channels,
            fade_frames,
            current_mix: mix,
            start_mix: mix,
            target_mix: mix,
            transition_frame: 0,
            transitioning: false,
        })
    }

    /// Begins a transition from the current interpolated position.
    pub fn set_target(&mut self, target: CrossfadeTarget) {
        let target_mix = target.wet_mix();
        if target_mix == self.target_mix && self.transitioning {
            return;
        }
        if target_mix == self.current_mix {
            self.target_mix = target_mix;
            self.transitioning = false;
            return;
        }
        self.start_mix = self.current_mix;
        self.target_mix = target_mix;
        self.transition_frame = 0;
        self.transitioning = true;
    }

    /// Blends equal-length, latency-aligned interleaved chunks.
    pub fn process(
        &mut self,
        dry: &[f32],
        wet: &[f32],
        output: &mut [f32],
    ) -> Result<usize, DspError> {
        ensure_aligned(dry.len(), self.channels)?;
        if wet.len() != dry.len() {
            return Err(DspError::OutputBufferTooSmall {
                required: dry.len(),
                actual: wet.len(),
            });
        }
        if output.len() < dry.len() {
            return Err(DspError::OutputBufferTooSmall {
                required: dry.len(),
                actual: output.len(),
            });
        }

        let channels = self.channels.as_usize();
        for ((dry_frame, wet_frame), output_frame) in dry
            .chunks_exact(channels)
            .zip(wet.chunks_exact(channels))
            .zip(output[..dry.len()].chunks_exact_mut(channels))
        {
            self.update_mix_for_frame();
            let theta = self.current_mix * FRAC_PI_2;
            let dry_gain = theta.cos();
            let wet_gain = theta.sin();
            for ((dry_sample, wet_sample), destination) in
                dry_frame.iter().zip(wet_frame).zip(output_frame)
            {
                *destination = dry_sample * dry_gain + wet_sample * wet_gain;
            }
        }
        Ok(dry.len() / channels)
    }

    fn update_mix_for_frame(&mut self) {
        if !self.transitioning {
            return;
        }
        let progress = if self.fade_frames == 1 {
            1.0
        } else {
            self.transition_frame as f32 / (self.fade_frames - 1) as f32
        };
        self.current_mix = self.start_mix + (self.target_mix - self.start_mix) * progress;
        self.transition_frame += 1;
        if self.transition_frame >= self.fade_frames {
            self.current_mix = self.target_mix;
            self.transitioning = false;
        }
    }

    /// Returns true while a requested endpoint transition is in progress.
    #[must_use]
    pub const fn is_transitioning(&self) -> bool {
        self.transitioning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_preserves_length_and_reaches_both_endpoints() {
        let channels = ChannelCount::new(1).expect("valid channels");
        let mut fade =
            EqualPowerCrossfade::new(channels, 5, CrossfadeTarget::Dry).expect("valid fade");
        fade.set_target(CrossfadeTarget::Wet);
        let mut output = [0.0; 5];
        fade.process(&[1.0; 5], &[0.0; 5], &mut output)
            .expect("valid buffers");
        assert!((output[0] - 1.0).abs() < 1.0e-6);
        assert!(output[4].abs() < 1.0e-6);
        assert!(!fade.is_transitioning());
    }

    #[test]
    fn equal_power_center_has_expected_gains() {
        let channels = ChannelCount::new(1).expect("valid channels");
        let mut fade =
            EqualPowerCrossfade::new(channels, 3, CrossfadeTarget::Dry).expect("valid fade");
        fade.set_target(CrossfadeTarget::Wet);
        let mut output = [0.0; 3];
        fade.process(&[1.0; 3], &[1.0; 3], &mut output)
            .expect("valid buffers");
        assert!((output[1] - 2.0_f32.sqrt()).abs() < 1.0e-6);
    }

    #[test]
    fn controlled_transition_has_no_large_sample_step() {
        let channels = ChannelCount::new(1).expect("valid channels");
        let mut fade =
            EqualPowerCrossfade::new(channels, 64, CrossfadeTarget::Dry).expect("valid fade");
        fade.set_target(CrossfadeTarget::Wet);
        let mut output = [0.0; 64];
        fade.process(&[1.0; 64], &[-1.0; 64], &mut output)
            .expect("valid buffers");
        let largest_step = output
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(largest_step < 0.04, "largest step was {largest_step}");
    }
}
