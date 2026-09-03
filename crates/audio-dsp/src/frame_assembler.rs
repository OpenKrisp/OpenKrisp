use audio_protocol::ChannelCount;

use crate::{DspError, interleaved::ensure_aligned};

/// Assembles arbitrary interleaved chunks into exact model-call frames.
///
/// Storage is allocated once. Each completed frame is borrowed by the callback before the same
/// storage is reused, so steady-state assembly neither allocates nor shifts pending samples.
#[derive(Debug)]
pub struct ModelFrameAssembler {
    channels: ChannelCount,
    frame_samples_per_channel: usize,
    pending: Vec<f32>,
    pending_samples: usize,
}

impl ModelFrameAssembler {
    /// Allocates storage for exactly one model frame.
    pub fn new(channels: ChannelCount, frame_samples_per_channel: usize) -> Result<Self, DspError> {
        if frame_samples_per_channel == 0 {
            return Err(DspError::ZeroModelFrameSize);
        }
        let samples = frame_samples_per_channel
            .checked_mul(channels.as_usize())
            .ok_or(DspError::ModelFrameSizeOverflow)?;
        Ok(Self {
            channels,
            frame_samples_per_channel,
            pending: vec![0.0; samples],
            pending_samples: 0,
        })
    }

    /// Adds a complete-frame-aligned chunk and invokes `on_frame` for every model frame produced.
    pub fn push<F>(&mut self, input: &[f32], mut on_frame: F) -> Result<usize, DspError>
    where
        F: FnMut(&[f32]),
    {
        ensure_aligned(input.len(), self.channels)?;
        let mut consumed = 0;
        let mut produced = 0;
        while consumed < input.len() {
            let available = self.pending.len() - self.pending_samples;
            let copied = available.min(input.len() - consumed);
            let pending_end = self.pending_samples + copied;
            self.pending[self.pending_samples..pending_end]
                .copy_from_slice(&input[consumed..consumed + copied]);
            self.pending_samples = pending_end;
            consumed += copied;

            if self.pending_samples == self.pending.len() {
                on_frame(&self.pending);
                self.pending_samples = 0;
                produced += 1;
            }
        }
        Ok(produced)
    }

    /// Drops an incomplete model frame after a timeline discontinuity.
    pub fn reset(&mut self) {
        self.pending_samples = 0;
        self.pending.fill(0.0);
    }

    /// Returns the incomplete audio-frame count retained for the next call.
    #[must_use]
    pub fn pending_frames(&self) -> usize {
        self.pending_samples / self.channels.as_usize()
    }

    /// Returns the exact number of samples per channel in one model call.
    #[must_use]
    pub const fn frame_samples_per_channel(&self) -> usize {
        self.frame_samples_per_channel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels(value: u16) -> ChannelCount {
        ChannelCount::new(value).expect("valid test channel count")
    }

    #[test]
    fn exact_and_multiple_frames_are_emitted() {
        let mut assembler = ModelFrameAssembler::new(channels(1), 4).expect("valid assembler");
        let mut frames = Vec::new();
        assert_eq!(
            assembler.push(&[0.0, 1.0, 2.0, 3.0], |frame| frames.push(frame.to_vec())),
            Ok(1)
        );
        assert_eq!(
            assembler.push(&[4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0], |frame| {
                frames.push(frame.to_vec());
            }),
            Ok(2)
        );
        assert_eq!(
            frames.concat(),
            (0..12).map(|value| value as f32).collect::<Vec<_>>()
        );
    }

    #[test]
    fn randomized_stereo_chunks_preserve_order_and_alignment() {
        let mut assembler = ModelFrameAssembler::new(channels(2), 7).expect("valid assembler");
        let input: Vec<f32> = (0..280).map(|sample| sample as f32).collect();
        let mut reconstructed = Vec::new();
        let mut offset = 0;
        let mut random = 0x1234_5678_u32;
        while offset < input.len() {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let chunk_frames = 1 + (random as usize % 19);
            let end = (offset + chunk_frames * 2).min(input.len());
            assembler
                .push(&input[offset..end], |frame| {
                    reconstructed.extend_from_slice(frame);
                })
                .expect("aligned randomized chunk");
            offset = end;
        }
        assert_eq!(assembler.pending_frames(), 0);
        assert_eq!(reconstructed, input);
    }

    #[test]
    fn reset_discards_partial_audio() {
        let mut assembler = ModelFrameAssembler::new(channels(1), 4).expect("valid assembler");
        assembler.push(&[1.0, 2.0], |_| {}).expect("valid chunk");
        assert_eq!(assembler.pending_frames(), 2);
        assembler.reset();

        let mut frame = Vec::new();
        assembler
            .push(&[3.0, 4.0, 5.0, 6.0], |samples| {
                frame.extend_from_slice(samples)
            })
            .expect("valid chunk");
        assert_eq!(frame, [3.0, 4.0, 5.0, 6.0]);
    }
}
