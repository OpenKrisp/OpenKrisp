use std::{fmt, path::Path};

use audio_protocol::DenoiseSettings;
use denoise_core::{DenoiseError, NoiseReducer, NoiseReducerDescriptor, NoiseReductionStats};
use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;
use static_assertions::assert_not_impl_any;

use crate::ModelAsset;

const ENABLED_POST_FILTER_BETA: f32 = 0.02;
const UNLIMITED_ATTENUATION_DB: f32 = 100.0;

assert_not_impl_any!(DfTract: Send, Sync);

/// Production adapter around the pinned Tract-backed DeepFilterNet implementation.
pub struct DfnNoiseReducer {
    asset: ModelAsset,
    params: DfParams,
    model: DfTract,
    descriptor: NoiseReducerDescriptor,
    settings: DenoiseSettings,
    input: Array2<f32>,
    output: Array2<f32>,
}

impl fmt::Debug for DfnNoiseReducer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DfnNoiseReducer")
            .field("asset", &self.asset)
            .field("descriptor", &self.descriptor)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl DfnNoiseReducer {
    /// Validates and loads a model asset into reusable planar buffers.
    pub fn load(
        model_path: impl AsRef<Path>,
        settings: DenoiseSettings,
    ) -> Result<Self, DenoiseError> {
        let asset = ModelAsset::load(model_path)?;
        Self::from_asset(asset, settings)
    }

    /// Loads an already validated model asset.
    pub fn from_asset(asset: ModelAsset, settings: DenoiseSettings) -> Result<Self, DenoiseError> {
        validate_settings(&settings)?;
        let params = DfParams::new(asset.path().to_path_buf())
            .map_err(|error| model_load_error(error.to_string()))?;
        let descriptor = asset.descriptor().clone();
        let model = construct_model(params.clone(), &descriptor, &settings)?;
        let shape = (
            descriptor.channels.as_usize(),
            descriptor.frame_samples_per_channel,
        );
        Ok(Self {
            asset,
            params,
            model,
            descriptor,
            settings,
            input: Array2::zeros(shape),
            output: Array2::zeros(shape),
        })
    }

    /// Runs low-level deterministic warm-up frames, then reconstructs the model for live audio.
    ///
    /// The pinned runtime deliberately short-circuits exact silence before inference, so the
    /// warm-up signal stays quiet but non-zero in order to exercise the complete model graph.
    pub fn warm_up(&mut self, frames: usize) -> Result<(), DenoiseError> {
        let sample_count = self.descriptor.frame_samples()?;
        let input = (0..sample_count)
            .map(|index| {
                if index.is_multiple_of(2) {
                    0.001
                } else {
                    -0.001
                }
            })
            .collect::<Vec<_>>();
        let mut output = vec![0.0; sample_count];
        for _ in 0..frames {
            self.process_planar(&input, &mut output)?;
        }
        self.reset()
    }

    #[must_use]
    pub const fn asset(&self) -> &ModelAsset {
        &self.asset
    }

    #[must_use]
    pub const fn settings(&self) -> &DenoiseSettings {
        &self.settings
    }
}

impl NoiseReducer for DfnNoiseReducer {
    fn descriptor(&self) -> &NoiseReducerDescriptor {
        &self.descriptor
    }

    fn process_planar(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<NoiseReductionStats, DenoiseError> {
        let expected = self.descriptor.frame_samples()?;
        if input.len() != expected {
            return Err(DenoiseError::InvalidFrameLength {
                expected,
                actual: input.len(),
            });
        }
        if output.len() != expected {
            return Err(DenoiseError::InvalidFrameLength {
                expected,
                actual: output.len(),
            });
        }
        if input.iter().any(|sample| !sample.is_finite()) {
            return Err(DenoiseError::NonFiniteInput);
        }

        for (destination, source) in self.input.iter_mut().zip(input) {
            *destination = *source;
        }
        let local_snr_db = self
            .model
            .process(self.input.view(), self.output.view_mut())
            .map_err(|error| DenoiseError::InferenceFailed(error.to_string()))?;
        if !local_snr_db.is_finite() || self.output.iter().any(|sample| !sample.is_finite()) {
            return Err(DenoiseError::NonFiniteOutput);
        }
        for (destination, source) in output.iter_mut().zip(self.output.iter()) {
            *destination = *source;
        }
        Ok(NoiseReductionStats {
            attenuation_db: None,
            speech_probability: None,
            local_snr_db: Some(local_snr_db),
        })
    }

    fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
        validate_settings(settings)?;
        apply_settings(&mut self.model, settings);
        self.settings = settings.clone();
        Ok(())
    }

    fn reset(&mut self) -> Result<(), DenoiseError> {
        let replacement = construct_model(self.params.clone(), &self.descriptor, &self.settings)
            .map_err(|error| DenoiseError::ResetFailed(error.to_string()))?;
        self.model = replacement;
        self.input.fill(0.0);
        self.output.fill(0.0);
        Ok(())
    }
}

fn construct_model(
    params: DfParams,
    expected: &NoiseReducerDescriptor,
    settings: &DenoiseSettings,
) -> Result<DfTract, DenoiseError> {
    let runtime = runtime_params(expected.channels.as_usize(), settings);
    let model =
        DfTract::new(params, &runtime).map_err(|error| model_load_error(error.to_string()))?;
    verify_model_metadata(&model, expected)?;
    Ok(model)
}

fn runtime_params(channels: usize, settings: &DenoiseSettings) -> RuntimeParams {
    RuntimeParams::default_with_ch(channels)
        .with_post_filter(if settings.post_filter_enabled {
            ENABLED_POST_FILTER_BETA
        } else {
            0.0
        })
        .with_atten_lim(
            settings
                .attenuation_limit_db
                .unwrap_or(UNLIMITED_ATTENUATION_DB),
        )
}

fn apply_settings(model: &mut DfTract, settings: &DenoiseSettings) {
    model.set_pf_beta(if settings.post_filter_enabled {
        ENABLED_POST_FILTER_BETA
    } else {
        0.0
    });
    model.set_atten_lim(
        settings
            .attenuation_limit_db
            .unwrap_or(UNLIMITED_ATTENUATION_DB),
    );
}

fn validate_settings(settings: &DenoiseSettings) -> Result<(), DenoiseError> {
    settings
        .validate()
        .map_err(|error| DenoiseError::InvalidSetting(error.to_string()))
}

fn verify_model_metadata(
    model: &DfTract,
    expected: &NoiseReducerDescriptor,
) -> Result<(), DenoiseError> {
    let actual_latency = model
        .fft_size
        .checked_sub(model.hop_size)
        .and_then(|base| {
            model
                .lookahead
                .checked_mul(model.hop_size)
                .and_then(|lookahead| base.checked_add(lookahead))
        })
        .ok_or(DenoiseError::InvalidDescriptor)?;
    let sample_rate_matches = usize::try_from(expected.sample_rate_hz.get()) == Ok(model.sr);
    if !sample_rate_matches
        || model.ch != expected.channels.as_usize()
        || model.hop_size != expected.frame_samples_per_channel
        || actual_latency != expected.algorithmic_latency_samples
    {
        return Err(DenoiseError::UnsupportedModelFormat);
    }
    Ok(())
}

fn model_load_error(message: impl Into<String>) -> DenoiseError {
    DenoiseError::ModelLoadFailed(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_model_path;

    #[test]
    fn adapter_validates_frames_processes_silence_and_resets() {
        let mut reducer = DfnNoiseReducer::load(default_model_path(), DenoiseSettings::default())
            .expect("packaged model loads");
        assert_eq!(reducer.descriptor().frame_samples_per_channel, 480);
        let mut output = vec![1.0; 480];
        assert!(matches!(
            reducer.process_planar(&[0.0; 479], &mut output),
            Err(DenoiseError::InvalidFrameLength { .. })
        ));
        assert_eq!(
            reducer.process_planar(&[f32::NAN; 480], &mut output),
            Err(DenoiseError::NonFiniteInput)
        );

        let stats = reducer
            .process_planar(&[0.0; 480], &mut output)
            .expect("silence processes");
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(stats.local_snr_db.is_some_and(f32::is_finite));

        let settings = DenoiseSettings {
            attenuation_limit_db: Some(24.0),
            post_filter_enabled: false,
            ..DenoiseSettings::default()
        };
        reducer
            .update_settings(&settings)
            .expect("verified settings update");
        assert_eq!(reducer.settings(), &settings);
        reducer.reset().expect("model reconstructs");
        output.fill(1.0);
        reducer
            .process_planar(&[0.0; 480], &mut output)
            .expect("reset model processes");
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));

        let fixture = (0..480)
            .map(|index| ((index % 31) as f32 - 15.0) * 0.002)
            .collect::<Vec<_>>();
        for _ in 0..16 {
            let stats = reducer
                .process_planar(&fixture, &mut output)
                .expect("repeated fixture frame processes");
            assert_eq!(output.len(), fixture.len());
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(stats.local_snr_db.is_some_and(f32::is_finite));
        }

        reducer.warm_up(2).expect("warm-up succeeds and resets");
        output.fill(1.0);
        reducer
            .process_planar(&[0.0; 480], &mut output)
            .expect("warm model starts from clean state");
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    }
}
