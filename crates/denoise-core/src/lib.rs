#![forbid(unsafe_code)]

//! Backend-neutral noise-reduction contracts and deterministic test implementation.

mod error;
mod fake;

use audio_protocol::{ChannelCount, DenoiseSettings, SampleRateHz};

pub use error::DenoiseError;
pub use fake::{FakeFailureMode, FakeNoiseReducer, FakeNoiseReducerConfig};

/// Model shape and fixed timing properties discovered during model loading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoiseReducerDescriptor {
    pub model_id: String,
    pub sample_rate_hz: SampleRateHz,
    pub channels: ChannelCount,
    pub frame_samples_per_channel: usize,
    pub algorithmic_latency_samples: usize,
}

impl NoiseReducerDescriptor {
    /// Returns the exact number of scalar samples in one planar model call.
    pub fn frame_samples(&self) -> Result<usize, DenoiseError> {
        if self.frame_samples_per_channel == 0 {
            return Err(DenoiseError::InvalidDescriptor);
        }
        self.frame_samples_per_channel
            .checked_mul(self.channels.as_usize())
            .ok_or(DenoiseError::InvalidDescriptor)
    }
}

/// Optional statistics returned by one successful inference call.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NoiseReductionStats {
    pub attenuation_db: Option<f32>,
    pub speech_probability: Option<f32>,
    pub local_snr_db: Option<f32>,
}

/// Object-safe contract confined to one DSP worker thread.
///
/// `Send` and `Sync` are intentionally not supertraits: the pinned DeepFilterNet processor is
/// neither, and each concrete processor stays on its constructing worker.
pub trait NoiseReducer {
    fn descriptor(&self) -> &NoiseReducerDescriptor;

    fn process_planar(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<NoiseReductionStats, DenoiseError>;

    fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError>;

    fn reset(&mut self) -> Result<(), DenoiseError>;
}
