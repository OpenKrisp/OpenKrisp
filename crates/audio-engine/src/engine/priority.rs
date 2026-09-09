use audio_protocol::{PipelineState, RouteId, RouteSnapshot};
use std::time::{Duration, Instant};

/// Incoming inference yields for two quiet seconds after new microphone deadline/queue pressure.
#[derive(Default)]
pub(super) struct RoutePriority {
    previous: Option<(RouteId, u64, u64)>,
    release_at: Option<Instant>,
}

impl RoutePriority {
    pub(super) fn observe(&mut self, routes: &[RouteSnapshot], now: Instant) -> bool {
        let Some(mic) = routes.iter().find(|route| {
            !route.config.is_incoming()
                && matches!(
                    route.state,
                    PipelineState::Starting | PipelineState::Running | PipelineState::Recovering
                )
        }) else {
            self.previous = None;
            self.release_at = None;
            return false;
        };
        let counts = (
            mic.metrics.missed_deadlines,
            mic.metrics
                .input_overruns
                .saturating_add(mic.metrics.output_underruns),
        );
        let changed = self
            .previous
            .as_ref()
            .is_some_and(|(id, deadlines, xruns)| {
                id == &mic.config.id && (counts.0 > *deadlines || counts.1 > *xruns)
            });
        if changed || mic.metrics.degraded || mic.state != PipelineState::Running {
            self.release_at = Some(now + Duration::from_secs(2));
        }
        self.previous = Some((mic.config.id.clone(), counts.0, counts.1));
        self.release_at.is_some_and(|deadline| now < deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::route_config;
    use super::*;

    #[test]
    fn microphone_pressure_holds_then_releases_without_latching_cumulative_counts() {
        let mut mic = RouteSnapshot::pending(route_config(), PipelineState::Starting);
        let mut priority = RoutePriority::default();
        let now = Instant::now();
        assert!(priority.observe(&[mic.clone()], now));
        mic.state = PipelineState::Running;
        assert!(priority.observe(&[mic.clone()], now + Duration::from_secs(1)));
        assert!(!priority.observe(&[mic.clone()], now + Duration::from_secs(3)));
        mic.metrics.missed_deadlines = 1;
        assert!(priority.observe(&[mic.clone()], now + Duration::from_secs(4)));
        assert!(!priority.observe(&[mic.clone()], now + Duration::from_secs(7)));
        mic.metrics.output_underruns = 1;
        assert!(priority.observe(&[mic.clone()], now + Duration::from_secs(8)));
        mic.state = PipelineState::Stopped;
        assert!(!priority.observe(&[mic], now + Duration::from_secs(8)));
    }
}
