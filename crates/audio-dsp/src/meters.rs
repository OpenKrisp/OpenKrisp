use crate::DspError;

/// Peak and RMS values after one block update.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LevelSnapshot {
    pub peak: f32,
    pub rms: f32,
}

/// Block meter with deterministic peak/RMS release decay.
#[derive(Clone, Copy, Debug)]
pub struct LevelMeter {
    decay_per_block: f32,
    snapshot: LevelSnapshot,
}

impl LevelMeter {
    /// Creates a meter. `decay_per_block` is a multiplier in `[0, 1]`.
    pub fn new(decay_per_block: f32) -> Result<Self, DspError> {
        if !decay_per_block.is_finite() || !(0.0..=1.0).contains(&decay_per_block) {
            return Err(DspError::InvalidMeterDecay);
        }
        Ok(Self {
            decay_per_block,
            snapshot: LevelSnapshot::default(),
        })
    }

    /// Measures finite samples and applies release decay to the previous snapshot.
    pub fn process(&mut self, samples: &[f32]) -> LevelSnapshot {
        let mut block_peak = 0.0_f32;
        let mut squared_sum = 0.0_f64;
        let mut finite_samples = 0_u64;
        for sample in samples.iter().copied().filter(|sample| sample.is_finite()) {
            block_peak = block_peak.max(sample.abs());
            squared_sum += f64::from(sample) * f64::from(sample);
            finite_samples += 1;
        }
        let block_rms = if finite_samples == 0 {
            0.0
        } else {
            (squared_sum / finite_samples as f64).sqrt() as f32
        };
        self.snapshot.peak = block_peak.max(self.snapshot.peak * self.decay_per_block);
        self.snapshot.rms = block_rms.max(self.snapshot.rms * self.decay_per_block);
        self.snapshot
    }

    /// Clears current and decaying levels.
    pub fn reset(&mut self) {
        self.snapshot = LevelSnapshot::default();
    }

    /// Returns the most recent decayed levels.
    #[must_use]
    pub const fn snapshot(self) -> LevelSnapshot {
        self.snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_signal_has_expected_peak_and_rms() {
        let mut meter = LevelMeter::new(0.5).expect("valid decay");
        let level = meter.process(&[1.0, -1.0, 0.0, 0.0]);
        assert_eq!(level.peak, 1.0);
        assert!((level.rms - 0.5_f32.sqrt()).abs() < 1.0e-6);

        let decayed = meter.process(&[0.0; 4]);
        assert_eq!(decayed.peak, 0.5);
        assert!((decayed.rms - level.rms * 0.5).abs() < 1.0e-6);
        meter.reset();
        assert_eq!(meter.snapshot(), LevelSnapshot::default());
    }

    #[test]
    fn non_finite_samples_do_not_poison_meter() {
        let mut meter = LevelMeter::new(0.0).expect("valid decay");
        assert_eq!(
            meter.process(&[f32::NAN, f32::INFINITY]),
            LevelSnapshot::default()
        );
    }
}
