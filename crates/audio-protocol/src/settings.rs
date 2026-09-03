use serde::{Deserialize, Serialize};

/// Runtime noise-reduction settings verified against the pinned backend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseSettings {
    pub enabled: bool,
    pub attenuation_limit_db: Option<f32>,
    pub post_filter_enabled: bool,
}

impl DenoiseSettings {
    /// Validates values that must be safe for every reducer backend.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if let Some(limit_db) = self.attenuation_limit_db
            && (!limit_db.is_finite() || limit_db < 0.0)
        {
            return Err(SettingsError::InvalidAttenuationLimit);
        }
        Ok(())
    }
}

impl Default for DenoiseSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            attenuation_limit_db: None,
            post_filter_enabled: true,
        }
    }
}

/// Invalid backend-neutral denoising setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("attenuation limit must be a finite, non-negative decibel value")]
    InvalidAttenuationLimit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_reject_non_finite_or_negative_attenuation() {
        for invalid in [f32::NAN, f32::INFINITY, -1.0] {
            let settings = DenoiseSettings {
                attenuation_limit_db: Some(invalid),
                ..DenoiseSettings::default()
            };
            assert_eq!(
                settings.validate(),
                Err(SettingsError::InvalidAttenuationLimit)
            );
        }
    }
}
