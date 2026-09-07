use std::time::Duration;

use audio_cpal::{AudioIoError, AudioIoErrorKind, StreamIssueKind};

/// Supervisor response to a compact stream or device failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryAction {
    /// The stream remains usable; retain the current route generation.
    Continue,
    /// Device configuration changed; negotiate and rebuild the route.
    Reconfigure,
    /// The endpoint may return; retry route construction with backoff.
    Retry,
    /// Retrying the same request cannot resolve the failure.
    Fail,
}

/// Bounded startup and runtime route-recovery policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteRecoveryPolicy {
    pub max_attempts: u8,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub activation_timeout: Duration,
    pub settle_period: Duration,
    pub poll_interval: Duration,
}

impl Default for RouteRecoveryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
            activation_timeout: Duration::from_secs(3),
            settle_period: Duration::from_millis(300),
            poll_interval: Duration::from_millis(10),
        }
    }
}

impl RouteRecoveryPolicy {
    pub(crate) fn validate(self) -> Result<(), &'static str> {
        if self.max_attempts == 0 {
            return Err("recovery attempts must be non-zero");
        }
        if self.initial_backoff.is_zero()
            || self.max_backoff < self.initial_backoff
            || self.activation_timeout.is_zero()
            || self.settle_period.is_zero()
            || self.poll_interval.is_zero()
            || self.settle_period >= self.activation_timeout
        {
            return Err("invalid route recovery timing policy");
        }
        Ok(())
    }
}

/// One numbered construction attempt and the delay before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryAttempt {
    pub number: u8,
    pub delay: Duration,
}

/// Finite exponential-backoff iterator owned by the supervisor.
#[derive(Clone, Copy, Debug)]
pub struct RecoveryBudget {
    policy: RouteRecoveryPolicy,
    next_attempt: u16,
}

impl RecoveryBudget {
    #[must_use]
    pub const fn new(policy: RouteRecoveryPolicy) -> Self {
        Self {
            policy,
            next_attempt: 1,
        }
    }
}

impl Iterator for RecoveryBudget {
    type Item = RecoveryAttempt;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_attempt > u16::from(self.policy.max_attempts) {
            return None;
        }
        let number = self.next_attempt as u8;
        self.next_attempt += 1;
        let delay = if number == 1 {
            Duration::ZERO
        } else {
            let shift = u32::from(number.saturating_sub(2)).min(31);
            self.policy
                .initial_backoff
                .saturating_mul(1_u32 << shift)
                .min(self.policy.max_backoff)
        };
        Some(RecoveryAttempt { number, delay })
    }
}

#[must_use]
pub const fn stream_issue_action(kind: StreamIssueKind) -> RecoveryAction {
    match kind {
        AudioIoErrorKind::DeviceChanged
        | AudioIoErrorKind::RealtimeDenied
        | AudioIoErrorKind::Xrun => RecoveryAction::Continue,
        AudioIoErrorKind::StreamInvalidated | AudioIoErrorKind::UnsupportedConfig => {
            RecoveryAction::Reconfigure
        }
        AudioIoErrorKind::DeviceBusy
        | AudioIoErrorKind::DeviceNotAvailable
        | AudioIoErrorKind::HostUnavailable
        | AudioIoErrorKind::ResourceExhausted
        | AudioIoErrorKind::Backend => RecoveryAction::Retry,
        AudioIoErrorKind::InvalidInput
        | AudioIoErrorKind::PermissionDenied
        | AudioIoErrorKind::UnsupportedOperation
        | AudioIoErrorKind::Other => RecoveryAction::Fail,
        _ => RecoveryAction::Fail,
    }
}

#[must_use]
pub const fn audio_error_action(error: &AudioIoError) -> RecoveryAction {
    match error {
        AudioIoError::Backend { kind, .. } => stream_issue_action(*kind),
        AudioIoError::NoDefaultDevice { .. } | AudioIoError::DeviceNotFound(_) => {
            RecoveryAction::Retry
        }
        AudioIoError::NoSupportedFormat { .. } => RecoveryAction::Reconfigure,
        AudioIoError::InvalidDeviceId(_) | AudioIoError::CallbackBufferOverflow => {
            RecoveryAction::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_budget_terminates_including_u8_max() {
        for max_attempts in 0..=u8::MAX {
            let policy = RouteRecoveryPolicy {
                max_attempts,
                ..RouteRecoveryPolicy::default()
            };
            let mut budget = RecoveryBudget::new(policy);
            for expected in 1..=max_attempts {
                assert_eq!(budget.next().expect("remaining attempt").number, expected);
            }
            assert_eq!(budget.next(), None);
            assert_eq!(budget.next(), None);
        }
    }

    #[test]
    fn recovery_budget_is_bounded_and_backoff_is_capped() {
        let policy = RouteRecoveryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(250),
            ..RouteRecoveryPolicy::default()
        };
        let attempts = RecoveryBudget::new(policy).collect::<Vec<_>>();
        assert_eq!(attempts.len(), 5);
        assert_eq!(attempts[0].delay, Duration::ZERO);
        assert_eq!(attempts[1].delay, Duration::from_millis(100));
        assert_eq!(attempts[2].delay, Duration::from_millis(200));
        assert_eq!(attempts[3].delay, Duration::from_millis(250));
        assert_eq!(attempts[4].delay, Duration::from_millis(250));
    }

    #[test]
    fn failure_categories_have_explicit_portable_actions() {
        assert_eq!(
            stream_issue_action(AudioIoErrorKind::StreamInvalidated),
            RecoveryAction::Reconfigure
        );
        assert_eq!(
            stream_issue_action(AudioIoErrorKind::DeviceNotAvailable),
            RecoveryAction::Retry
        );
        assert_eq!(
            stream_issue_action(AudioIoErrorKind::DeviceChanged),
            RecoveryAction::Continue
        );
        assert_eq!(
            stream_issue_action(AudioIoErrorKind::PermissionDenied),
            RecoveryAction::Fail
        );
    }

    #[test]
    fn policy_rejects_unbounded_or_incoherent_values() {
        let policy = RouteRecoveryPolicy {
            max_attempts: 0,
            ..RouteRecoveryPolicy::default()
        };
        assert!(policy.validate().is_err());

        let policy = RouteRecoveryPolicy {
            settle_period: RouteRecoveryPolicy::default().activation_timeout,
            ..RouteRecoveryPolicy::default()
        };
        assert!(policy.validate().is_err());
    }
}
