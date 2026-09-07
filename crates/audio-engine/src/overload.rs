const DEADLINE_WINDOW_FRAMES: usize = 8;
const MISSES_TO_DEGRADE: usize = 4;
const DEGRADED_PROBE_INTERVAL_FRAMES: usize = 100;
const HEALTHY_PROBES_TO_RECOVER: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverloadTransition {
    None,
    EnteredDegraded,
    ExitedDegraded,
}

/// Tracks sustained deadline pressure and schedules sparse recovery probes while degraded.
#[derive(Clone, Debug, Default)]
pub(crate) struct OverloadGuard {
    misses: [bool; DEADLINE_WINDOW_FRAMES],
    history_len: usize,
    cursor: usize,
    degraded: bool,
    frames_until_probe: usize,
    healthy_probes: usize,
    trial_active: bool,
    reset_requested: bool,
}

impl OverloadGuard {
    #[must_use]
    pub(crate) const fn is_degraded(&self) -> bool {
        self.degraded
    }

    /// Returns whether the model should run for this frame.
    pub(crate) fn should_process_model(&mut self) -> bool {
        if !self.degraded {
            return true;
        }
        if self.trial_active {
            return true;
        }
        if self.frames_until_probe == 0 {
            self.trial_active = true;
            self.reset_requested = true;
            true
        } else {
            self.frames_until_probe -= 1;
            false
        }
    }

    pub(crate) fn take_reset_request(&mut self) -> bool {
        std::mem::take(&mut self.reset_requested)
    }

    pub(crate) fn enter_degraded(&mut self) {
        self.degraded = true;
        self.frames_until_probe = DEGRADED_PROBE_INTERVAL_FRAMES;
        self.healthy_probes = 0;
        self.trial_active = false;
        self.reset_requested = false;
    }

    /// Records one attempted inference. Failures count as misses regardless of elapsed time.
    pub(crate) fn record_attempt(&mut self, missed: bool) -> OverloadTransition {
        if self.degraded {
            if missed {
                self.enter_degraded();
            } else {
                self.healthy_probes += 1;
                if self.healthy_probes >= HEALTHY_PROBES_TO_RECOVER {
                    self.reset();
                    return OverloadTransition::ExitedDegraded;
                }
            }
            return OverloadTransition::None;
        }

        self.misses[self.cursor] = missed;
        self.cursor = (self.cursor + 1) % DEADLINE_WINDOW_FRAMES;
        self.history_len = (self.history_len + 1).min(DEADLINE_WINDOW_FRAMES);
        let misses = self.misses[..self.history_len]
            .iter()
            .filter(|missed| **missed)
            .count();
        if self.history_len == DEADLINE_WINDOW_FRAMES && misses >= MISSES_TO_DEGRADE {
            self.enter_degraded();
            OverloadTransition::EnteredDegraded
        } else {
            OverloadTransition::None
        }
    }

    pub(crate) fn reset(&mut self) {
        self.misses.fill(false);
        self.history_len = 0;
        self.cursor = 0;
        self.degraded = false;
        self.frames_until_probe = 0;
        self.healthy_probes = 0;
        self.trial_active = false;
        self.reset_requested = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_spikes_do_not_degrade() {
        let mut guard = OverloadGuard::default();
        for missed in [false, false, true, false, false, false, false, false] {
            assert_eq!(guard.record_attempt(missed), OverloadTransition::None);
        }
        assert!(!guard.is_degraded());
    }

    #[test]
    fn sustained_misses_degrade_and_skip_most_model_work() {
        let mut guard = OverloadGuard::default();
        let mut transition = OverloadTransition::None;
        for missed in [true, false, true, false, true, false, true, false] {
            transition = guard.record_attempt(missed);
        }
        assert_eq!(transition, OverloadTransition::EnteredDegraded);
        assert!(guard.is_degraded());

        assert!((0..100).all(|_| !guard.should_process_model()));
        for index in 0..HEALTHY_PROBES_TO_RECOVER {
            assert!(guard.should_process_model());
            assert_eq!(guard.take_reset_request(), index == 0);
            guard.record_attempt(false);
        }
        assert!(!guard.is_degraded());
    }

    #[test]
    fn healthy_probes_leave_degraded_mode() {
        let mut guard = OverloadGuard::default();
        for _ in 0..DEADLINE_WINDOW_FRAMES {
            guard.record_attempt(true);
        }
        assert!(guard.is_degraded());

        let mut transition = OverloadTransition::None;
        for _ in 0..HEALTHY_PROBES_TO_RECOVER {
            while !guard.should_process_model() {}
            transition = guard.record_attempt(false);
        }
        assert_eq!(transition, OverloadTransition::ExitedDegraded);
        assert!(!guard.is_degraded());
        assert!(guard.should_process_model());
    }
}
