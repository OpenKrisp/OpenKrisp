use audio_protocol::PipelineState;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Out-of-band cancellation and progress for the thread owning a physical route.
#[derive(Clone)]
pub(crate) struct MonitorControl {
    pub(crate) cancelled: Arc<AtomicBool>,
    progress: Arc<dyn Fn(PipelineState) + Send + Sync>,
}

impl Default for MonitorControl {
    fn default() -> Self {
        Self::new(|_| {})
    }
}

impl MonitorControl {
    pub(crate) fn new(progress: impl Fn(PipelineState) + Send + Sync + 'static) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(progress),
        }
    }
    pub(crate) fn with_cancellation(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            progress: Arc::new(|_| {}),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub(crate) fn publish(&self, state: PipelineState) {
        (self.progress)(state);
    }
    pub(crate) fn check(&self) -> Result<(), crate::HeadlessMonitorError> {
        if self.is_cancelled() {
            Err(crate::HeadlessMonitorError::Cancelled)
        } else {
            Ok(())
        }
    }
    pub(crate) fn wait(&self, duration: Duration) -> Result<(), crate::HeadlessMonitorError> {
        let deadline = Instant::now() + duration;
        loop {
            self.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            thread::sleep(remaining.min(Duration::from_millis(5)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_interrupts_a_long_retry_delay() {
        let control = MonitorControl::default();
        let worker_control = control.clone();
        let join = thread::spawn(move || worker_control.wait(Duration::from_secs(30)));
        control.cancel();
        assert!(matches!(
            join.join().expect("join"),
            Err(crate::HeadlessMonitorError::Cancelled)
        ));
    }
}
