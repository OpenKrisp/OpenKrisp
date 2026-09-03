use crate::DspError;

/// Counts safety substitutions performed by one limiter call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LimiterStats {
    pub limited_samples: usize,
    pub non_finite_samples: usize,
}

/// Conservative hard safety limiter and non-finite sanitizer.
#[derive(Clone, Copy, Debug)]
pub struct SafetyLimiter {
    threshold: f32,
}

impl SafetyLimiter {
    /// Creates a limiter with a normalized full-scale threshold in `(0, 1]`.
    pub fn new(threshold: f32) -> Result<Self, DspError> {
        if !threshold.is_finite() || threshold <= 0.0 || threshold > 1.0 {
            return Err(DspError::InvalidLimiterThreshold);
        }
        Ok(Self { threshold })
    }

    /// Replaces NaN/infinity with silence and clamps values beyond the threshold.
    pub fn process_in_place(&self, samples: &mut [f32]) -> LimiterStats {
        let mut stats = LimiterStats::default();
        for sample in samples {
            if !sample.is_finite() {
                *sample = 0.0;
                stats.non_finite_samples += 1;
            } else {
                let limited = sample.clamp(-self.threshold, self.threshold);
                if limited != *sample {
                    stats.limited_samples += 1;
                    *sample = limited;
                }
            }
        }
        stats
    }

    /// Returns the normalized full-scale threshold.
    #[must_use]
    pub const fn threshold(self) -> f32 {
        self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_sanitizes_and_bounds_output() {
        let limiter = SafetyLimiter::new(0.95).expect("valid threshold");
        let mut samples = [f32::NAN, f32::INFINITY, -2.0, -0.5, 0.5, 2.0];
        let stats = limiter.process_in_place(&mut samples);
        assert_eq!(stats.non_finite_samples, 2);
        assert_eq!(stats.limited_samples, 2);
        assert_eq!(samples, [0.0, 0.0, -0.95, -0.5, 0.5, 0.95]);
        assert!(samples.iter().all(|sample| sample.is_finite()));
    }
}
