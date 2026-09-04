use audio_protocol::PipelineState;

/// Rejected authoritative route-state transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid route-state transition from {from:?} to {to:?}")]
pub struct StateTransitionError {
    pub from: PipelineState,
    pub to: PipelineState,
}

/// Small supervisor-owned lifecycle state machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteStateMachine {
    state: PipelineState,
}

impl RouteStateMachine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: PipelineState::Stopped,
        }
    }

    #[must_use]
    pub const fn state(self) -> PipelineState {
        self.state
    }

    pub fn transition(&mut self, next: PipelineState) -> Result<(), StateTransitionError> {
        if !is_allowed(self.state, next) {
            return Err(StateTransitionError {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        Ok(())
    }
}

fn is_allowed(from: PipelineState, to: PipelineState) -> bool {
    matches!(
        (from, to),
        (PipelineState::Stopped, PipelineState::Starting)
            | (
                PipelineState::Starting,
                PipelineState::Running | PipelineState::Failed | PipelineState::Stopping
            )
            | (
                PipelineState::Running,
                PipelineState::Reconfiguring
                    | PipelineState::Recovering
                    | PipelineState::Failed
                    | PipelineState::Stopping
            )
            | (
                PipelineState::Reconfiguring,
                PipelineState::Running
                    | PipelineState::Recovering
                    | PipelineState::Failed
                    | PipelineState::Stopping
            )
            | (
                PipelineState::Recovering,
                PipelineState::Running | PipelineState::Failed | PipelineState::Stopping
            )
            | (
                PipelineState::Failed,
                PipelineState::Starting | PipelineState::Stopping
            )
            | (PipelineState::Stopping, PipelineState::Stopped)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_start_and_stop_path_is_explicit() {
        let mut state = RouteStateMachine::new();
        state
            .transition(PipelineState::Starting)
            .expect("start transition");
        state
            .transition(PipelineState::Running)
            .expect("ready transition");
        state
            .transition(PipelineState::Stopping)
            .expect("stop transition");
        state
            .transition(PipelineState::Stopped)
            .expect("stopped transition");
        assert_eq!(state.state(), PipelineState::Stopped);
    }

    #[test]
    fn duplicate_and_invalid_transitions_are_rejected() {
        let mut state = RouteStateMachine::new();
        assert_eq!(
            state.transition(PipelineState::Running),
            Err(StateTransitionError {
                from: PipelineState::Stopped,
                to: PipelineState::Running,
            })
        );
        state
            .transition(PipelineState::Starting)
            .expect("start transition");
        assert!(state.transition(PipelineState::Starting).is_err());
    }

    #[test]
    fn failed_route_can_be_stopped_or_retried() {
        let mut state = RouteStateMachine::new();
        state.transition(PipelineState::Starting).expect("starting");
        state.transition(PipelineState::Failed).expect("failed");
        state.transition(PipelineState::Starting).expect("retry");
        state
            .transition(PipelineState::Failed)
            .expect("failed again");
        state.transition(PipelineState::Stopping).expect("stopping");
        state.transition(PipelineState::Stopped).expect("stopped");
    }
}
