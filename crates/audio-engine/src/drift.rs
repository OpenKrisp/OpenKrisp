const DEFAULT_MAX_CORRECTION_PPM: i32 = 1_000;
const DEFAULT_MAX_SLEW_PPM_PER_UPDATE: f64 = 10.0;
const DEFAULT_PROPORTIONAL_GAIN: f64 = 3.0;
const DEFAULT_INTEGRAL_GAIN: f64 = 0.005;
const DEFAULT_FILL_FILTER_ALPHA: f64 = 0.05;

/// Startup-only tuning for the slow output-ring occupancy controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DriftControllerConfig {
    pub target_fill_frames: usize,
    pub max_correction_ppm: i32,
    pub max_slew_ppm_per_update: f64,
    pub proportional_gain: f64,
    pub integral_gain: f64,
    pub fill_filter_alpha: f64,
}

impl DriftControllerConfig {
    pub(crate) fn for_target(target_fill_frames: usize) -> Self {
        Self {
            target_fill_frames,
            max_correction_ppm: DEFAULT_MAX_CORRECTION_PPM,
            max_slew_ppm_per_update: DEFAULT_MAX_SLEW_PPM_PER_UPDATE,
            proportional_gain: DEFAULT_PROPORTIONAL_GAIN,
            integral_gain: DEFAULT_INTEGRAL_GAIN,
            fill_filter_alpha: DEFAULT_FILL_FILTER_ALPHA,
        }
    }
}

/// A bounded, slew-limited PI controller driven by output-ring occupancy.
#[derive(Clone, Debug)]
pub(crate) struct DriftController {
    config: DriftControllerConfig,
    filtered_fill_frames: f64,
    filter_initialized: bool,
    integral_error: f64,
    correction_ppm: f64,
}

impl DriftController {
    pub(crate) fn new(config: DriftControllerConfig) -> Self {
        debug_assert!(config.target_fill_frames > 0);
        debug_assert!(config.max_correction_ppm > 0);
        debug_assert!(config.max_slew_ppm_per_update > 0.0);
        debug_assert!(config.proportional_gain >= 0.0);
        debug_assert!(config.integral_gain > 0.0);
        debug_assert!((0.0..=1.0).contains(&config.fill_filter_alpha));
        Self {
            config,
            filtered_fill_frames: 0.0,
            filter_initialized: false,
            integral_error: 0.0,
            correction_ppm: 0.0,
        }
    }

    /// Observes the current fill and returns the next relative output-rate correction.
    pub(crate) fn update(&mut self, current_fill_frames: usize) -> i32 {
        let observed_fill = current_fill_frames as f64;
        if self.filter_initialized {
            self.filtered_fill_frames +=
                self.config.fill_filter_alpha * (observed_fill - self.filtered_fill_frames);
        } else {
            self.filtered_fill_frames = observed_fill;
            self.filter_initialized = true;
        }

        // A low output queue needs more output frames, hence the target-minus-current sign.
        let error = self.config.target_fill_frames as f64 - self.filtered_fill_frames;
        let integral_limit = f64::from(self.config.max_correction_ppm) / self.config.integral_gain;
        let candidate_integral =
            (self.integral_error + error).clamp(-integral_limit, integral_limit);
        let candidate = self
            .config
            .proportional_gain
            .mul_add(error, self.config.integral_gain * candidate_integral);
        let max_correction = f64::from(self.config.max_correction_ppm);

        // Stop winding up while saturated, but always permit integration that unwinds saturation.
        if candidate.abs() <= max_correction || candidate.signum() != error.signum() || error == 0.0
        {
            self.integral_error = candidate_integral;
        }
        let desired = self
            .config
            .proportional_gain
            .mul_add(error, self.config.integral_gain * self.integral_error)
            .clamp(-max_correction, max_correction);
        let lower = self.correction_ppm - self.config.max_slew_ppm_per_update;
        let upper = self.correction_ppm + self.config.max_slew_ppm_per_update;
        self.correction_ppm = desired.clamp(lower, upper);
        self.correction_ppm.round() as i32
    }

    pub(crate) fn reset(&mut self) {
        self.filtered_fill_frames = 0.0;
        self.filter_initialized = false;
        self.integral_error = 0.0;
        self.correction_ppm = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL_FRAMES: f64 = 480.0;
    const TARGET_FILL: usize = 1_920;
    const CAPACITY: f64 = 9_600.0;

    struct DriftRun {
        fill_frames: f64,
        correction_ppm: i32,
        minimum_fill_frames: f64,
        maximum_fill_frames: f64,
    }

    fn simulate(source_drift_ppm: i32, seconds: usize) -> DriftRun {
        let mut controller = DriftController::new(DriftControllerConfig::for_target(TARGET_FILL));
        let mut fill_frames = TARGET_FILL as f64;
        let mut correction_ppm = 0;
        let mut minimum_fill_frames = fill_frames;
        let mut maximum_fill_frames = fill_frames;
        for _ in 0..seconds * 100 {
            let source_ratio = 1.0 + f64::from(source_drift_ppm) / 1_000_000.0;
            let correction_ratio = 1.0 + f64::from(correction_ppm) / 1_000_000.0;
            fill_frames += MODEL_FRAMES.mul_add(source_ratio * correction_ratio, -MODEL_FRAMES);
            minimum_fill_frames = minimum_fill_frames.min(fill_frames);
            maximum_fill_frames = maximum_fill_frames.max(fill_frames);
            correction_ppm = controller.update(fill_frames.round().max(0.0) as usize);
        }
        DriftRun {
            fill_frames,
            correction_ppm,
            minimum_fill_frames,
            maximum_fill_frames,
        }
    }

    #[test]
    fn positive_and_negative_clock_drift_converge_near_target() {
        for drift_ppm in [-100, 100] {
            let run = simulate(drift_ppm, 180);
            assert!(
                (run.fill_frames - TARGET_FILL as f64).abs() < 4.0,
                "{drift_ppm} ppm ended at {} frames with {} ppm correction",
                run.fill_frames,
                run.correction_ppm
            );
            assert!(
                (run.correction_ppm + drift_ppm).abs() <= 5,
                "{drift_ppm} ppm settled at {} ppm correction",
                run.correction_ppm
            );
            assert!(run.minimum_fill_frames > 0.0);
            assert!(run.maximum_fill_frames < CAPACITY);
        }
    }

    #[test]
    fn wider_clock_drift_stays_bounded_and_converges() {
        for drift_ppm in [-750, 750] {
            let run = simulate(drift_ppm, 240);
            assert!((run.fill_frames - TARGET_FILL as f64).abs() < 12.0);
            assert!(run.correction_ppm.abs() <= DEFAULT_MAX_CORRECTION_PPM);
            assert!(run.minimum_fill_frames > 0.0);
            assert!(run.maximum_fill_frames < CAPACITY);
        }
    }

    #[test]
    fn controller_tracks_a_clock_rate_step_without_a_queue_reset() {
        let mut controller = DriftController::new(DriftControllerConfig::for_target(TARGET_FILL));
        let mut fill_frames = TARGET_FILL as f64;
        let mut correction_ppm = 0;
        for (source_drift_ppm, seconds) in [(100, 120), (-100, 180)] {
            for _ in 0..seconds * 100 {
                let source_ratio = 1.0 + f64::from(source_drift_ppm) / 1_000_000.0;
                let correction_ratio = 1.0 + f64::from(correction_ppm) / 1_000_000.0;
                fill_frames += MODEL_FRAMES.mul_add(source_ratio * correction_ratio, -MODEL_FRAMES);
                correction_ppm = controller.update(fill_frames.round().max(0.0) as usize);
            }
        }

        assert!((fill_frames - TARGET_FILL as f64).abs() < 4.0);
        assert!((correction_ppm - 100).abs() <= 5);
    }

    #[test]
    fn correction_is_slew_limited_bounded_and_resettable() {
        let mut controller = DriftController::new(DriftControllerConfig::for_target(TARGET_FILL));
        let first = controller.update(0);
        let second = controller.update(0);
        assert_eq!(first, 10);
        assert_eq!(second, 20);
        for _ in 0..1_000 {
            controller.update(0);
        }
        assert_eq!(controller.update(0), DEFAULT_MAX_CORRECTION_PPM);

        controller.reset();
        assert_eq!(controller.update(TARGET_FILL), 0);
    }
}
