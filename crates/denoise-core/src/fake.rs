use std::{num::NonZeroU64, thread, time::Duration};

use audio_protocol::DenoiseSettings;

use crate::{DenoiseError, NoiseReducer, NoiseReducerDescriptor, NoiseReductionStats};

/// Deterministic inference-failure schedule for [`FakeNoiseReducer`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FakeFailureMode {
    #[default]
    Never,
    Always,
    OnCall(u64),
    Every(NonZeroU64),
}

impl FakeFailureMode {
    fn should_fail(self, call: u64) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::OnCall(target) => call == target,
            Self::Every(interval) => call.is_multiple_of(interval.get()),
        }
    }
}

/// Startup-only configuration for the fake reducer.
#[derive(Clone, Debug)]
pub struct FakeNoiseReducerConfig {
    pub descriptor: NoiseReducerDescriptor,
    pub settings: DenoiseSettings,
    pub enabled_gain: f32,
    pub processing_delay: Duration,
    pub failure_mode: FakeFailureMode,
    pub stats: NoiseReductionStats,
}

/// Predictable reducer for worker and simulation tests.
#[derive(Debug)]
pub struct FakeNoiseReducer {
    config: FakeNoiseReducerConfig,
    calls: u64,
}

impl FakeNoiseReducer {
    /// Validates the descriptor, settings, and finite gain once at construction.
    pub fn new(config: FakeNoiseReducerConfig) -> Result<Self, DenoiseError> {
        config.descriptor.frame_samples()?;
        config
            .settings
            .validate()
            .map_err(|error| DenoiseError::InvalidSetting(error.to_string()))?;
        if !config.enabled_gain.is_finite() {
            return Err(DenoiseError::InvalidSetting(
                "fake gain must be finite".to_owned(),
            ));
        }
        Ok(Self { config, calls: 0 })
    }

    /// Returns the number of valid-sized process attempts since the last reset.
    #[must_use]
    pub const fn calls(&self) -> u64 {
        self.calls
    }
}

impl NoiseReducer for FakeNoiseReducer {
    fn descriptor(&self) -> &NoiseReducerDescriptor {
        &self.config.descriptor
    }

    fn process_planar(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<NoiseReductionStats, DenoiseError> {
        let expected = self.config.descriptor.frame_samples()?;
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

        self.calls += 1;
        if !self.config.processing_delay.is_zero() {
            thread::sleep(self.config.processing_delay);
        }
        if self.config.failure_mode.should_fail(self.calls) {
            return Err(DenoiseError::InjectedFailure { call: self.calls });
        }

        let gain = if self.config.settings.enabled {
            self.config.enabled_gain
        } else {
            1.0
        };
        for (source, destination) in input.iter().zip(output.iter_mut()) {
            *destination = source * gain;
        }
        if output.iter().any(|sample| !sample.is_finite()) {
            return Err(DenoiseError::NonFiniteOutput);
        }
        Ok(self.config.stats)
    }

    fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
        settings
            .validate()
            .map_err(|error| DenoiseError::InvalidSetting(error.to_string()))?;
        self.config.settings = settings.clone();
        Ok(())
    }

    fn reset(&mut self) -> Result<(), DenoiseError> {
        self.calls = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use audio_protocol::{ChannelCount, SampleRateHz};

    use super::*;

    fn config() -> FakeNoiseReducerConfig {
        FakeNoiseReducerConfig {
            descriptor: NoiseReducerDescriptor {
                model_id: "fake-phase-1".to_owned(),
                sample_rate_hz: SampleRateHz::new(48_000).expect("valid rate"),
                channels: ChannelCount::new(1).expect("valid channels"),
                frame_samples_per_channel: 4,
                algorithmic_latency_samples: 4,
            },
            settings: DenoiseSettings::default(),
            enabled_gain: 0.5,
            processing_delay: Duration::ZERO,
            failure_mode: FakeFailureMode::Never,
            stats: NoiseReductionStats {
                attenuation_db: Some(6.0),
                speech_probability: Some(0.75),
                local_snr_db: None,
            },
        }
    }

    #[test]
    fn fake_validates_dimensions_and_finite_input() {
        let mut reducer = FakeNoiseReducer::new(config()).expect("valid fake");
        assert_eq!(
            reducer.process_planar(&[0.0; 3], &mut [0.0; 4]),
            Err(DenoiseError::InvalidFrameLength {
                expected: 4,
                actual: 3
            })
        );
        assert_eq!(
            reducer.process_planar(&[0.0, f32::NAN, 0.0, 0.0], &mut [0.0; 4]),
            Err(DenoiseError::NonFiniteInput)
        );
    }

    #[test]
    fn settings_and_reset_are_reproducible() {
        let mut reducer = FakeNoiseReducer::new(config()).expect("valid fake");
        let input = [1.0, 0.5, -0.5, -1.0];
        let mut output = [0.0; 4];
        let stats = reducer
            .process_planar(&input, &mut output)
            .expect("successful fake call");
        assert_eq!(output, [0.5, 0.25, -0.25, -0.5]);
        assert_eq!(stats.attenuation_db, Some(6.0));

        reducer
            .update_settings(&DenoiseSettings {
                enabled: false,
                ..DenoiseSettings::default()
            })
            .expect("valid setting update");
        reducer
            .process_planar(&input, &mut output)
            .expect("successful bypass call");
        assert_eq!(output, input);
        reducer.reset().expect("fake reset succeeds");
        assert_eq!(reducer.calls(), 0);
    }

    #[test]
    fn failure_and_processing_delay_are_configurable() {
        let mut fake_config = config();
        fake_config.failure_mode = FakeFailureMode::OnCall(2);
        fake_config.processing_delay = Duration::from_millis(2);
        let mut reducer = FakeNoiseReducer::new(fake_config).expect("valid fake");
        let input = [0.0; 4];
        let mut output = [0.0; 4];
        let started = Instant::now();
        reducer
            .process_planar(&input, &mut output)
            .expect("first call succeeds");
        assert!(started.elapsed() >= Duration::from_millis(2));
        assert_eq!(
            reducer.process_planar(&input, &mut output),
            Err(DenoiseError::InjectedFailure { call: 2 })
        );
    }
}
