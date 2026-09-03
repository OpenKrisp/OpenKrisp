use audio_protocol::{ChannelCount, SampleRateHz};
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters,
    audioadapter_buffers::direct::InterleavedSlice,
};

use crate::{DspError, interleaved::ensure_aligned};

const MAX_RELATIVE_RATIO: f64 = 1.01;

/// Progress made by one bounded streaming resampler call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResampleIo {
    pub input_frames_consumed: usize,
    pub output_frames_written: usize,
}

#[derive(Debug)]
enum Backend {
    Passthrough,
    Sinc(Async<f32>),
}

/// Fixed-rate streaming resampler with preallocated input and output staging buffers.
///
/// Input may arrive in any complete-frame-aligned chunk size. A call may consume only part of its
/// input when the caller's output slice is full; the returned [`ResampleIo`] makes backpressure
/// explicit. The underlying adjustable sinc implementation leaves room for Phase 3 clock
/// correction, but this Phase 1 API keeps the nominal ratio fixed.
#[derive(Debug)]
pub struct StreamingResampler {
    channels: ChannelCount,
    input_rate_hz: SampleRateHz,
    output_rate_hz: SampleRateHz,
    input_block_frames: usize,
    backend: Backend,
    input_buffer: Vec<f32>,
    input_frames_filled: usize,
    output_buffer: Vec<f32>,
    output_frames_ready: usize,
    output_frame_offset: usize,
    output_delay_frames: usize,
    output_frames_max: usize,
}

impl StreamingResampler {
    /// Constructs the resampler and all storage used by steady-state processing.
    pub fn new(
        input_rate_hz: SampleRateHz,
        output_rate_hz: SampleRateHz,
        channels: ChannelCount,
        chunk_size_frames: usize,
    ) -> Result<Self, DspError> {
        if chunk_size_frames == 0 {
            return Err(DspError::ZeroResamplerChunkSize);
        }
        let channel_count = channels.as_usize();
        if input_rate_hz == output_rate_hz {
            return Ok(Self {
                channels,
                input_rate_hz,
                output_rate_hz,
                input_block_frames: chunk_size_frames,
                backend: Backend::Passthrough,
                input_buffer: Vec::new(),
                input_frames_filled: 0,
                output_buffer: Vec::new(),
                output_frames_ready: 0,
                output_frame_offset: 0,
                output_delay_frames: 0,
                output_frames_max: chunk_size_frames,
            });
        }

        let ratio = f64::from(output_rate_hz.get()) / f64::from(input_rate_hz.get());
        let resampler = Async::<f32>::new_sinc(
            ratio,
            MAX_RELATIVE_RATIO,
            &SincInterpolationParameters::default(),
            chunk_size_frames,
            channel_count,
            FixedAsync::Input,
        )
        .map_err(|error| DspError::ResamplerConstruction(error.to_string()))?;
        let input_frames = resampler.input_frames_next();
        let output_frames_max = resampler.output_frames_max();
        let output_delay_frames = resampler.output_delay();
        let input_samples = input_frames
            .checked_mul(channel_count)
            .ok_or(DspError::ResamplerSizeOverflow)?;
        let output_samples = output_frames_max
            .checked_mul(channel_count)
            .ok_or(DspError::ResamplerSizeOverflow)?;

        Ok(Self {
            channels,
            input_rate_hz,
            output_rate_hz,
            input_block_frames: input_frames,
            backend: Backend::Sinc(resampler),
            input_buffer: vec![0.0; input_samples],
            input_frames_filled: 0,
            output_buffer: vec![0.0; output_samples],
            output_frames_ready: 0,
            output_frame_offset: 0,
            output_delay_frames,
            output_frames_max,
        })
    }

    /// Processes as much interleaved input and output as the caller-owned slices permit.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<ResampleIo, DspError> {
        ensure_aligned(input.len(), self.channels)?;
        ensure_aligned(output.len(), self.channels)?;
        if matches!(self.backend, Backend::Passthrough) {
            return Ok(self.process_passthrough(input, output));
        }

        let channels = self.channels.as_usize();
        let input_frames = input.len() / channels;
        let output_capacity_frames = output.len() / channels;
        let mut progress = ResampleIo::default();

        loop {
            self.drain_ready_output(output, &mut progress);
            if progress.output_frames_written == output_capacity_frames {
                break;
            }

            if self.output_frames_ready > 0 {
                break;
            }

            if progress.input_frames_consumed < input_frames {
                let needed = self.input_block_frames - self.input_frames_filled;
                let available = input_frames - progress.input_frames_consumed;
                let copied_frames = needed.min(available);
                let source_start = progress.input_frames_consumed * channels;
                let source_end = source_start + copied_frames * channels;
                let destination_start = self.input_frames_filled * channels;
                let destination_end = destination_start + copied_frames * channels;
                self.input_buffer[destination_start..destination_end]
                    .copy_from_slice(&input[source_start..source_end]);
                self.input_frames_filled += copied_frames;
                progress.input_frames_consumed += copied_frames;
            }

            if self.input_frames_filled == self.input_block_frames {
                self.process_staged_block()?;
                continue;
            }

            break;
        }
        Ok(progress)
    }

    fn process_passthrough(&self, input: &[f32], output: &mut [f32]) -> ResampleIo {
        let channels = self.channels.as_usize();
        let frames = (input.len() / channels).min(output.len() / channels);
        let samples = frames * channels;
        output[..samples].copy_from_slice(&input[..samples]);
        ResampleIo {
            input_frames_consumed: frames,
            output_frames_written: frames,
        }
    }

    fn drain_ready_output(&mut self, output: &mut [f32], progress: &mut ResampleIo) {
        let channels = self.channels.as_usize();
        let output_capacity_frames = output.len() / channels;
        let pending_frames = self.output_frames_ready - self.output_frame_offset;
        let available_frames = output_capacity_frames - progress.output_frames_written;
        let copied_frames = pending_frames.min(available_frames);
        if copied_frames == 0 {
            return;
        }
        let source_start = self.output_frame_offset * channels;
        let source_end = source_start + copied_frames * channels;
        let destination_start = progress.output_frames_written * channels;
        let destination_end = destination_start + copied_frames * channels;
        output[destination_start..destination_end]
            .copy_from_slice(&self.output_buffer[source_start..source_end]);
        self.output_frame_offset += copied_frames;
        progress.output_frames_written += copied_frames;
        if self.output_frame_offset == self.output_frames_ready {
            self.output_frame_offset = 0;
            self.output_frames_ready = 0;
        }
    }

    fn process_staged_block(&mut self) -> Result<(), DspError> {
        let Backend::Sinc(resampler) = &mut self.backend else {
            return Ok(());
        };
        let channels = self.channels.as_usize();
        let input = InterleavedSlice::new(
            self.input_buffer.as_slice(),
            channels,
            self.input_block_frames,
        )
        .map_err(|error| DspError::Resampling(error.to_string()))?;
        let mut output = InterleavedSlice::new_mut(
            self.output_buffer.as_mut_slice(),
            channels,
            self.output_frames_max,
        )
        .map_err(|error| DspError::Resampling(error.to_string()))?;
        let (_, produced) = resampler
            .process_into_buffer(&input, &mut output, None)
            .map_err(|error| DspError::Resampling(error.to_string()))?;
        self.input_frames_filled = 0;
        self.output_frame_offset = 0;
        self.output_frames_ready = produced;
        Ok(())
    }

    /// Clears partial input, pending output, and all filter history.
    pub fn reset(&mut self) {
        if let Backend::Sinc(resampler) = &mut self.backend {
            resampler.reset();
        }
        self.input_buffer.fill(0.0);
        self.output_buffer.fill(0.0);
        self.input_frames_filled = 0;
        self.output_frames_ready = 0;
        self.output_frame_offset = 0;
    }

    /// Returns the group delay measured in output-rate frames.
    #[must_use]
    pub const fn output_delay_frames(&self) -> usize {
        self.output_delay_frames
    }

    /// Returns the maximum frames a single internal block can produce.
    #[must_use]
    pub const fn output_frames_max(&self) -> usize {
        self.output_frames_max
    }

    /// Returns incomplete input retained for the next call.
    #[must_use]
    pub const fn pending_input_frames(&self) -> usize {
        self.input_frames_filled
    }

    /// Returns produced output retained until the caller provides more capacity.
    #[must_use]
    pub const fn pending_output_frames(&self) -> usize {
        self.output_frames_ready - self.output_frame_offset
    }

    /// Returns the nominal source and destination rates.
    #[must_use]
    pub const fn rates_hz(&self) -> (SampleRateHz, SampleRateHz) {
        (self.input_rate_hz, self.output_rate_hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(value: u32) -> SampleRateHz {
        SampleRateHz::new(value).expect("valid test rate")
    }

    fn mono() -> ChannelCount {
        ChannelCount::new(1).expect("valid channel count")
    }

    fn resample_one_second(input_rate: u32, output_rate: u32) -> Vec<f32> {
        let chunk_size = (input_rate / 100) as usize;
        let mut resampler =
            StreamingResampler::new(rate(input_rate), rate(output_rate), mono(), chunk_size)
                .expect("supported test resampler");
        let input = vec![0.5; input_rate as usize];
        let mut output = Vec::new();
        let mut offset = 0;
        let mut random = 0x9e37_79b9_u32;
        let mut scratch = vec![0.0; resampler.output_frames_max() + 17];
        while offset < input.len() || resampler.pending_output_frames() > 0 {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let chunk = 1 + random as usize % 997;
            let end = (offset + chunk).min(input.len());
            let progress = resampler
                .process(&input[offset..end], &mut scratch)
                .expect("valid resampling buffers");
            offset += progress.input_frames_consumed;
            output.extend_from_slice(&scratch[..progress.output_frames_written]);
            assert!(progress.input_frames_consumed > 0 || progress.output_frames_written > 0);
        }
        assert_eq!(resampler.pending_input_frames(), 0);
        output
    }

    #[test]
    fn required_device_rates_preserve_duration() {
        for (input_rate, output_rate) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 48_000),
            (48_000, 48_000),
        ] {
            let output = resample_one_second(input_rate, output_rate);
            assert!(
                output.len().abs_diff(output_rate as usize) <= 8,
                "{input_rate} -> {output_rate} Hz produced {} frames",
                output.len()
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn constant_signal_is_stable_after_filter_delay() {
        let output = resample_one_second(44_100, 48_000);
        let settled = &output[1_000..output.len() - 1_000];
        assert!(settled.iter().all(|sample| (*sample - 0.5).abs() < 2.0e-3));
    }

    #[test]
    fn reset_clears_partial_and_filter_history() {
        let mut resampler = StreamingResampler::new(rate(44_100), rate(48_000), mono(), 441)
            .expect("supported test resampler");
        let mut output = vec![0.0; resampler.output_frames_max()];
        resampler
            .process(&vec![1.0; 441], &mut output)
            .expect("valid resampling buffers");
        resampler
            .process(&[1.0; 17], &mut [])
            .expect("valid partial input");
        resampler.reset();
        assert_eq!(resampler.pending_input_frames(), 0);
        assert_eq!(resampler.pending_output_frames(), 0);

        output.fill(1.0);
        let progress = resampler
            .process(&[0.0; 441], &mut output)
            .expect("valid resampling buffers");
        assert!(
            output[..progress.output_frames_written]
                .iter()
                .all(|sample| sample.abs() < f32::EPSILON)
        );
    }

    #[test]
    fn impulse_response_is_finite_and_matches_reported_delay() {
        let mut resampler = StreamingResampler::new(rate(48_000), rate(44_100), mono(), 480)
            .expect("supported test resampler");
        let reported_delay = resampler.output_delay_frames();
        let input_frames = 4_800;
        let mut input = vec![0.0; input_frames];
        input[0] = 1.0;
        let mut output = Vec::new();
        let mut scratch = vec![0.0; resampler.output_frames_max()];
        let mut offset = 0;
        while offset < input.len() || resampler.pending_output_frames() > 0 {
            let progress = resampler
                .process(&input[offset..], &mut scratch)
                .expect("valid resampling buffers");
            offset += progress.input_frames_consumed;
            output.extend_from_slice(&scratch[..progress.output_frames_written]);
            assert!(progress.input_frames_consumed > 0 || progress.output_frames_written > 0);
        }

        assert!(output.iter().all(|sample| sample.is_finite()));
        let impulse_index = output
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.abs()
                    .partial_cmp(&right.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(index, _)| index)
            .expect("resampler produced output");
        assert!(
            impulse_index.abs_diff(reported_delay) <= 1,
            "impulse peaked at frame {impulse_index}, reported delay is {reported_delay}"
        );
    }
}
