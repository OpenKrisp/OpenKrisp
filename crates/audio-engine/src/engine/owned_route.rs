use std::sync::RwLock;

use super::*;
use crate::lifecycle::MonitorControl;

enum RouteCommand {
    Bypass(bool),
    Settings(DenoiseSettings),
}

/// The supervisor only exchanges bounded commands and immutable observations. Native stream
/// handles stay on this owner thread, and the non-Send reducer stays on its separate DSP thread.
pub(super) struct OwnedRoute {
    route_id: RouteId,
    snapshot: Arc<RwLock<RouteSnapshot>>,
    control: MonitorControl,
    commands: SyncSender<RouteCommand>,
    owner: Option<JoinHandle<()>>,
}

impl OwnedRoute {
    pub(super) fn spawn<F>(config: RouteConfig, factory: F) -> Result<Self, OperationFailure>
    where
        F: FnOnce(MonitorControl) -> Result<Box<dyn ManagedRoute>, OperationFailure>
            + Send
            + 'static,
    {
        let route_id = config.id.clone();
        let snapshot = Arc::new(RwLock::new(RouteSnapshot::pending(
            config,
            PipelineState::Starting,
        )));
        let progress = snapshot.clone();
        let control = MonitorControl::new(move |state| {
            let mut current = progress.write().unwrap_or_else(|error| error.into_inner());
            if current.state != PipelineState::Stopping {
                current.state = state;
            }
        });
        let (commands, receiver) = sync_channel(8);
        let owner_snapshot = snapshot.clone();
        let owner_control = control.clone();
        let owner = thread::Builder::new()
            .name("openkrisp-route".to_owned())
            .spawn(move || {
                run_owner(factory, receiver, owner_snapshot, owner_control);
            })
            .map_err(|_| {
                OperationFailure::new(
                    ErrorCode::WorkerTerminated,
                    "The audio route owner could not start.",
                    false,
                )
            })?;
        Ok(Self {
            route_id,
            snapshot,
            control,
            commands,
            owner: Some(owner),
        })
    }

    fn send(&self, command: RouteCommand) -> Result<(), OperationFailure> {
        self.commands.try_send(command).map_err(|error| {
            OperationFailure::from_engine(match error {
                TrySendError::Full(_) => EngineError::CommandQueueFull,
                TrySendError::Disconnected(_) => EngineError::CommandChannelDisconnected,
            })
        })
    }

    fn join(&mut self) -> Result<(), OperationFailure> {
        if let Some(owner) = self.owner.take() {
            owner.join().map_err(|_| {
                OperationFailure::new(
                    ErrorCode::WorkerTerminated,
                    "The audio route owner stopped unexpectedly.",
                    false,
                )
            })?;
        }
        Ok(())
    }
}

impl ManagedRoute for OwnedRoute {
    fn request_stop(&mut self) {
        let _ = self.stop();
    }

    fn route_id(&self) -> &RouteId {
        &self.route_id
    }

    fn snapshot(&mut self) -> RouteSnapshot {
        if self.owner.as_ref().is_some_and(JoinHandle::is_finished)
            && let Err(failure) = self.join()
        {
            let mut current = self
                .snapshot
                .write()
                .unwrap_or_else(|error| error.into_inner());
            current.state = PipelineState::Failed;
            current.fault = Some(failure.error);
        }
        let mut current = self
            .snapshot
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if self.owner.is_none() && self.control.is_cancelled() {
            current.state = PipelineState::Stopped;
        }
        current.clone()
    }

    fn set_bypass(&mut self, bypassed: bool) -> Result<(), OperationFailure> {
        self.send(RouteCommand::Bypass(bypassed))
    }
    fn update_settings(&mut self, settings: DenoiseSettings) -> Result<(), OperationFailure> {
        self.send(RouteCommand::Settings(settings))
    }

    fn stop(&mut self) -> Result<(), OperationFailure> {
        // Synchronize the state and cancellation with publication: a late startup completion
        // can never revive a route after Stop has been processed.
        let mut current = self
            .snapshot
            .write()
            .unwrap_or_else(|error| error.into_inner());
        self.control.cancel();
        if self.owner.is_none() {
            current.state = PipelineState::Stopped;
        } else if current.state != PipelineState::Stopped {
            current.state = PipelineState::Stopping;
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), OperationFailure> {
        self.stop()?;
        self.join()
    }
}

impl Drop for OwnedRoute {
    fn drop(&mut self) {
        self.control.cancel();
        let _ = self.join();
    }
}

fn run_owner<F>(
    factory: F,
    commands: Receiver<RouteCommand>,
    snapshot: Arc<RwLock<RouteSnapshot>>,
    control: MonitorControl,
) where
    F: FnOnce(MonitorControl) -> Result<Box<dyn ManagedRoute>, OperationFailure>,
{
    let mut route = match factory(control.clone()) {
        Ok(route) => route,
        Err(failure) => {
            let mut current = snapshot.write().unwrap_or_else(|error| error.into_inner());
            if control.is_cancelled() {
                current.state = PipelineState::Stopped;
            } else {
                current.state = PipelineState::Failed;
                current.fault = Some(failure.error);
            }
            return;
        }
    };
    let mut command_fault = None;
    while !control.is_cancelled() {
        route.poll_health();
        let mut observation = route.snapshot();
        if observation.fault.is_none() {
            observation.fault = command_fault.clone();
        }
        {
            let mut current = snapshot.write().unwrap_or_else(|error| error.into_inner());
            if !control.is_cancelled() {
                *current = observation;
            }
        }
        if control.is_cancelled() {
            break;
        }
        let result = match commands.recv_timeout(DEFAULT_POLL_INTERVAL) {
            Ok(RouteCommand::Bypass(bypassed)) => route.set_bypass(bypassed),
            Ok(RouteCommand::Settings(settings)) => route.update_settings(settings),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if let Err(failure) = result {
            command_fault = Some(failure.error);
        }
    }
    let result = route.shutdown();
    let mut stopped = route.snapshot();
    // Drop all native resources before advertising Stopped.
    drop(route);
    stopped.state = PipelineState::Stopped;
    if let Err(failure) = result {
        stopped.fault = Some(failure.error);
    }
    *snapshot.write().unwrap_or_else(|error| error.into_inner()) = stopped;
}

#[cfg(test)]
mod tests {
    use super::super::tests::{FakeRoute, route_config};
    use super::*;
    use std::sync::{Mutex, atomic::AtomicUsize};

    #[test]
    fn a_failed_owner_can_still_be_stopped_after_its_thread_has_exited() {
        let mut owner = OwnedRoute::spawn(route_config(), |_| {
            Err(OperationFailure::new(
                ErrorCode::Denoise,
                "injected startup failure",
                false,
            ))
        })
        .expect("owner");
        let deadline = Instant::now() + Duration::from_secs(2);
        while owner
            .owner
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            assert!(Instant::now() < deadline, "owner finishes");
            thread::yield_now();
        }
        assert_eq!(owner.snapshot().state, PipelineState::Failed);
        owner.stop().expect("failed owner stops");
        assert_eq!(owner.snapshot().state, PipelineState::Stopped);
        assert!(owner.snapshot().fault.is_some());
    }

    struct RecoveringRoute {
        inner: FakeRoute,
        control: MonitorControl,
        entered: SyncSender<()>,
    }

    impl ManagedRoute for RecoveringRoute {
        fn route_id(&self) -> &RouteId {
            self.inner.route_id()
        }
        fn snapshot(&mut self) -> RouteSnapshot {
            self.inner.snapshot()
        }
        fn set_bypass(&mut self, bypassed: bool) -> Result<(), OperationFailure> {
            self.inner.set_bypass(bypassed)
        }
        fn update_settings(&mut self, settings: DenoiseSettings) -> Result<(), OperationFailure> {
            self.inner.update_settings(settings)
        }
        fn stop(&mut self) -> Result<(), OperationFailure> {
            self.inner.stop()
        }
        fn poll_health(&mut self) {
            self.control.publish(PipelineState::Recovering);
            self.entered.send(()).expect("recovery entered");
            let _ = self.control.wait(Duration::from_secs(30));
        }
    }

    #[test]
    fn recovery_progress_is_observable_and_snapshot_reads_do_not_wait_for_recovery() {
        let config = route_config();
        let (entered, receiver) = sync_channel(1);
        let stops = Arc::new(AtomicUsize::new(0));
        let owner_stops = stops.clone();
        let mut owner = OwnedRoute::spawn(config.clone(), move |control| {
            Ok(Box::new(RecoveringRoute {
                inner: FakeRoute {
                    route_id: config.id.clone(),
                    snapshot: Mutex::new(RouteSnapshot::pending(config, PipelineState::Running)),
                    stops: owner_stops,
                },
                control,
                entered,
            }))
        })
        .expect("owner");
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("recovery entered");
        assert_eq!(owner.snapshot().state, PipelineState::Recovering);
        owner.stop().expect("stop accepted during recovery");
        assert!(matches!(
            owner.snapshot().state,
            PipelineState::Stopping | PipelineState::Stopped
        ));
        owner.shutdown().expect("owner joined");
        assert_eq!(owner.snapshot().state, PipelineState::Stopped);
        assert_eq!(stops.load(Ordering::Relaxed), 1);
    }
}
