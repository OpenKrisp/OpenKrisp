use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use audio_cpal::{
    AudioIoError, AudioIoErrorKind, CallbackMetricsSnapshot, CpalHost, DeviceSelection,
    StreamIssueKind, StreamIssueSnapshot,
};
use audio_protocol::{
    DenoiseSettings, DestinationEndpoint, DeviceInventory, EngineErrorDto, EngineEvent,
    EngineObservation, EngineSnapshot, ErrorCode, ModelInfo, PipelineState, ProcessingMode,
    RouteConfig, RouteId, RouteSnapshot, SourceEndpoint, StreamDiagnostics,
};
use denoise_dfn::default_model_path;

use crate::{EngineSnapshotStore, HeadlessMonitor, HeadlessMonitorConfig, HeadlessMonitorError};

mod owned_route;
use owned_route::OwnedRoute;

const DEFAULT_COMMAND_CAPACITY: usize = 16;
const DEFAULT_EVENT_CAPACITY: usize = 64;
const DEFAULT_METRICS_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_TARGET_LATENCY_MS: u16 = 1_000;

/// Model location inside a packaged desktop application's resource directory.
pub const DEFAULT_MODEL_RESOURCE_PATH: &str = "models/DeepFilterNet3_ll_onnx.tar.gz";

/// Construction options for the application-owned audio supervisor.
#[derive(Clone, Debug)]
pub struct AudioEngineConfig {
    pub model_path: PathBuf,
    pub command_capacity: usize,
    pub event_capacity: usize,
    pub metrics_interval: Duration,
    pub poll_interval: Duration,
}

impl Default for AudioEngineConfig {
    fn default() -> Self {
        Self {
            model_path: default_model_path(),
            command_capacity: DEFAULT_COMMAND_CAPACITY,
            event_capacity: DEFAULT_EVENT_CAPACITY,
            metrics_interval: DEFAULT_METRICS_INTERVAL,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }
}

impl AudioEngineConfig {
    fn validate(&self) -> Result<(), AudioEngineStartError> {
        if self.command_capacity == 0 {
            return Err(AudioEngineStartError::InvalidConfiguration(
                "engine command capacity must be non-zero",
            ));
        }
        if self.event_capacity == 0 {
            return Err(AudioEngineStartError::InvalidConfiguration(
                "engine event capacity must be non-zero",
            ));
        }
        if self.metrics_interval.is_zero() || self.poll_interval.is_zero() {
            return Err(AudioEngineStartError::InvalidConfiguration(
                "engine timing intervals must be non-zero",
            ));
        }
        Ok(())
    }
}

/// Bounded commands accepted by the engine control thread.
#[derive(Clone, Debug)]
pub enum EngineCommand {
    RefreshDevices,
    StartRoute(RouteConfig),
    StopRoute(RouteId),
    UpdateDenoise {
        route_id: RouteId,
        settings: DenoiseSettings,
    },
    SetBypass {
        route_id: RouteId,
        bypassed: bool,
    },
    Shutdown,
}

impl EngineCommand {
    fn validate(&self) -> Result<(), EngineError> {
        match self {
            Self::StartRoute(config) => validate_route_config(config),
            Self::UpdateDenoise { settings, .. } => settings
                .validate()
                .map_err(|_| EngineError::InvalidConfiguration("invalid denoise settings")),
            Self::RefreshDevices | Self::StopRoute(_) | Self::SetBypass { .. } | Self::Shutdown => {
                Ok(())
            }
        }
    }
}

/// Immediate command-submission and engine-lifecycle failures.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("invalid engine configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("route {route_id} cannot accept this command while {state:?}")]
    InvalidState {
        route_id: RouteId,
        state: PipelineState,
    },
    #[error("route was not found: {0}")]
    RouteNotFound(RouteId),
    #[error("engine command queue is full")]
    CommandQueueFull,
    #[error("engine command channel is disconnected")]
    CommandChannelDisconnected,
    #[error("engine shutdown is in progress")]
    ShutdownInProgress,
    #[error("engine supervisor thread terminated unexpectedly")]
    SupervisorTerminated,
}

impl EngineError {
    /// Converts an internal failure into a stable, sanitized control-plane payload.
    #[must_use]
    pub fn to_dto(&self) -> EngineErrorDto {
        match self {
            Self::InvalidConfiguration(message) => {
                EngineErrorDto::new(ErrorCode::InvalidConfiguration, *message)
            }
            Self::InvalidState { state, .. } => EngineErrorDto::new(
                ErrorCode::InvalidState,
                format!("The route cannot accept this command while {state:?}."),
            ),
            Self::RouteNotFound(_) => EngineErrorDto::new(
                ErrorCode::InvalidState,
                "The requested route does not exist.",
            ),
            Self::CommandQueueFull => EngineErrorDto::new(
                ErrorCode::CommandQueueFull,
                "The audio engine is busy. Try again shortly.",
            ),
            Self::CommandChannelDisconnected | Self::SupervisorTerminated => EngineErrorDto::new(
                ErrorCode::WorkerTerminated,
                "The audio engine is unavailable.",
            ),
            Self::ShutdownInProgress => EngineErrorDto::new(
                ErrorCode::ShutdownInProgress,
                "The audio engine is shutting down.",
            ),
        }
    }
}

/// Failure to construct the bounded supervisor resources or native thread.
#[derive(Debug, thiserror::Error)]
pub enum AudioEngineStartError {
    #[error("invalid audio engine configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("failed to spawn audio engine supervisor: {0}")]
    Thread(#[source] std::io::Error),
}

/// Cloneable, non-blocking control-plane handle.
#[derive(Clone, Debug)]
pub struct AudioEngineHandle {
    command_tx: SyncSender<EngineCommand>,
    snapshot: EngineSnapshotStore,
    shutdown_requested: Arc<AtomicBool>,
}

impl AudioEngineHandle {
    /// Validates and enqueues a command without waiting for route initialization or teardown.
    pub fn try_send(&self, command: EngineCommand) -> Result<(), EngineError> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(EngineError::ShutdownInProgress);
        }
        command.validate()?;
        if matches!(command, EngineCommand::Shutdown) {
            self.request_shutdown();
            return Ok(());
        }
        self.command_tx
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => EngineError::CommandQueueFull,
                TrySendError::Disconnected(_) => EngineError::CommandChannelDisconnected,
            })
    }

    /// Returns the latest immutable reconciliation snapshot.
    #[must_use]
    pub fn snapshot(&self) -> EngineSnapshot {
        self.snapshot.snapshot()
    }

    /// Requests deterministic route teardown even when the regular command queue is full.
    pub fn request_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::Release);
    }
}

/// Owns the supervisor thread and the single bounded event receiver.
#[must_use = "dropping the owner shuts down and joins the audio engine"]
pub struct AudioEngine {
    handle: AudioEngineHandle,
    events: Option<Receiver<EngineObservation>>,
    supervisor: Option<JoinHandle<()>>,
}

impl AudioEngine {
    /// Starts the production CPAL-backed supervisor.
    pub fn start(config: AudioEngineConfig) -> Result<Self, AudioEngineStartError> {
        let model_path = config.model_path.clone();
        Self::start_with_driver(config, move || Box::new(ProductionDriver { model_path }))
    }

    fn start_with_driver<F>(
        config: AudioEngineConfig,
        driver_factory: F,
    ) -> Result<Self, AudioEngineStartError>
    where
        F: FnOnce() -> Box<dyn EngineDriver> + Send + 'static,
    {
        config.validate()?;
        let (command_tx, command_rx) = sync_channel(config.command_capacity);
        let (event_tx, event_rx) = sync_channel(config.event_capacity);
        let snapshot = EngineSnapshotStore::default();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let thread_snapshot = snapshot.clone();
        let thread_shutdown = shutdown_requested.clone();
        let supervisor = thread::Builder::new()
            .name("openkrisp-engine".to_owned())
            .spawn(move || {
                run_supervisor(
                    config,
                    command_rx,
                    event_tx,
                    thread_snapshot,
                    thread_shutdown,
                    driver_factory(),
                );
            })
            .map_err(AudioEngineStartError::Thread)?;

        Ok(Self {
            handle: AudioEngineHandle {
                command_tx,
                snapshot,
                shutdown_requested,
            },
            events: Some(event_rx),
            supervisor: Some(supervisor),
        })
    }

    #[must_use]
    pub fn handle(&self) -> AudioEngineHandle {
        self.handle.clone()
    }

    /// Transfers the only event receiver to the desktop event bridge.
    pub fn take_event_receiver(&mut self) -> Option<Receiver<EngineObservation>> {
        self.events.take()
    }

    /// Requests shutdown and joins the supervisor and every route-owned resource it controls.
    pub fn shutdown(&mut self) -> Result<(), EngineError> {
        self.handle.request_shutdown();
        match self.supervisor.take() {
            Some(supervisor) => supervisor
                .join()
                .map_err(|_| EngineError::SupervisorTerminated),
            None => Ok(()),
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

trait ManagedRoute {
    fn route_id(&self) -> &RouteId;
    fn snapshot(&mut self) -> RouteSnapshot;
    fn set_bypass(&mut self, bypassed: bool) -> Result<(), OperationFailure>;
    fn update_settings(&mut self, settings: DenoiseSettings) -> Result<(), OperationFailure>;
    fn stop(&mut self) -> Result<(), OperationFailure>;
    fn poll_health(&mut self) {}
    fn shutdown(&mut self) -> Result<(), OperationFailure> {
        self.stop()
    }
}

trait EngineDriver: Send {
    fn inventory(&mut self) -> Result<DeviceInventory, OperationFailure>;
    fn start_route(
        &mut self,
        config: RouteConfig,
    ) -> Result<Box<dyn ManagedRoute>, OperationFailure>;
}

struct ProductionDriver {
    model_path: PathBuf,
}

impl EngineDriver for ProductionDriver {
    fn inventory(&mut self) -> Result<DeviceInventory, OperationFailure> {
        CpalHost::new_default()
            .inventory()
            .map_err(OperationFailure::from_audio_io)
    }

    fn start_route(
        &mut self,
        config: RouteConfig,
    ) -> Result<Box<dyn ManagedRoute>, OperationFailure> {
        validate_route_config(&config).map_err(OperationFailure::from_engine)?;
        let target_latency_ms = config.target_latency_ms.get();
        let output_capacity_ms = target_latency_ms.saturating_mul(2).max(200);
        let model_path = self.model_path.clone();
        let owner = OwnedRoute::spawn(config.clone(), move |control| {
            control.check().map_err(OperationFailure::from_headless)?;
            // Inventory and preparation may touch the OS; keep them on the cancellable owner.
            let inventory = CpalHost::new_default()
                .inventory()
                .map_err(OperationFailure::from_audio_io)?;
            let (inventory, endpoint_session) =
                prepare_endpoint_session(&config, inventory, &control)?;
            let prepared = platform_routing::current_router()
                .prepare_route(&config, &inventory)
                .map_err(OperationFailure::from_routing)?;
            let bypassed = config.processing_mode == ProcessingMode::AlignedBypass;
            let monitor_config = HeadlessMonitorConfig {
                route_id: config.id.clone(),
                input: DeviceSelection::Id(prepared.input_device_id),
                output: DeviceSelection::Id(prepared.output_device_id),
                platform_route: Some(config.clone()),
                model_path,
                denoise: config.denoise.clone(),
                initially_bypassed: bypassed,
                target_latency_ms,
                output_capacity_ms,
                ..HeadlessMonitorConfig::default()
            };
            let monitor = HeadlessMonitor::start_controlled(monitor_config, control)
                .map_err(OperationFailure::from_headless)?;
            Ok(Box::new(HeadlessRoute {
                config,
                monitor,
                bypassed,
                endpoint_session,
                endpoint_fault: None,
            }))
        })?;
        Ok(Box::new(owner))
    }
}

struct HeadlessRoute {
    config: RouteConfig,
    monitor: HeadlessMonitor,
    bypassed: bool,
    // Declared after monitor so streams are torn down before their owned OS endpoints.
    endpoint_session: Option<platform_routing::pipewire::PipeWireSession>,
    endpoint_fault: Option<EngineErrorDto>,
}

fn prepare_endpoint_session(
    config: &RouteConfig,
    inventory: DeviceInventory,
    control: &crate::lifecycle::MonitorControl,
) -> Result<
    (
        DeviceInventory,
        Option<platform_routing::pipewire::PipeWireSession>,
    ),
    OperationFailure,
> {
    #[cfg(target_os = "linux")]
    if matches!(&config.destination, DestinationEndpoint::VirtualMicrophone(id) if id.as_str() == platform_routing::linux::PIPEWIRE_ENDPOINT_ID)
    {
        use audio_protocol::EndpointAvailability;
        use platform_routing::{RoutingError, linux::LinuxRouter, pipewire::PipeWireSession};
        let router = LinuxRouter;
        router
            .validate_source(config, &inventory)
            .map_err(OperationFailure::from_routing)?;
        match router.endpoint(&inventory).availability {
            EndpointAvailability::Ready => return Ok((inventory, None)),
            EndpointAvailability::Invalid => {
                return Err(OperationFailure::from_routing(
                    RoutingError::InvalidEndpoint,
                ));
            }
            EndpointAvailability::Unsupported => {
                return Err(OperationFailure::from_routing(
                    RoutingError::UnsupportedPlatform,
                ));
            }
            EndpointAvailability::Missing => (),
        }
        control.check().map_err(OperationFailure::from_headless)?;
        let mut session = PipeWireSession::start().map_err(OperationFailure::from_routing)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            control
                .wait(Duration::from_millis(50))
                .map_err(OperationFailure::from_headless)?;
            session
                .check_running()
                .map_err(OperationFailure::from_routing)?;
            // CPAL's PipeWire host holds a device snapshot; reconstruct it after creating nodes.
            let current = CpalHost::new_default()
                .inventory()
                .map_err(OperationFailure::from_audio_io)?;
            if router.endpoint(&current).availability == EndpointAvailability::Ready {
                return Ok((current, Some(session)));
            }
            if Instant::now() >= deadline {
                return Err(OperationFailure::from_routing(
                    RoutingError::EndpointCreationFailed,
                ));
            }
        }
    }
    let _ = (config, control);
    Ok((inventory, None))
}

impl ManagedRoute for HeadlessRoute {
    fn poll_health(&mut self) {
        if let Some(session) = self.endpoint_session.as_mut()
            && let Err(error) = session.check_running()
        {
            let _ = self.monitor.stop();
            self.endpoint_session.take();
            self.endpoint_fault = Some(error.to_dto());
            return;
        }
        self.monitor.poll_health();
    }

    fn route_id(&self) -> &RouteId {
        &self.config.id
    }

    fn snapshot(&mut self) -> RouteSnapshot {
        let snapshot = self.monitor.snapshot();
        RouteSnapshot {
            config: self.config.clone(),
            state: if self.endpoint_fault.is_some() {
                PipelineState::Failed
            } else {
                snapshot.state
            },
            bypassed: self.bypassed,
            input_device: Some(snapshot.input_device),
            output_device: Some(snapshot.output_device),
            input_format: Some(snapshot.input_format),
            output_format: Some(snapshot.output_format),
            model: Some(ModelInfo {
                id: snapshot.model.model_id,
                sample_rate_hz: snapshot.model.sample_rate_hz,
                channels: snapshot.model.channels.get(),
                frame_samples_per_channel: saturating_u32(snapshot.model.frame_samples_per_channel),
                algorithmic_latency_samples: saturating_u32(
                    snapshot.model.algorithmic_latency_samples,
                ),
            }),
            metrics: snapshot.metrics,
            input_stream: stream_diagnostics(snapshot.input_callbacks, snapshot.input_issues),
            output_stream: stream_diagnostics(snapshot.output_callbacks, snapshot.output_issues),
            model_errors: snapshot.model_errors,
            recovery_attempts: snapshot.recovery_attempts,
            successful_recoveries: snapshot.successful_recoveries,
            uptime_seconds: snapshot.uptime_seconds,
            fault: self.endpoint_fault.clone().or_else(|| {
                (snapshot.state == PipelineState::Failed).then(|| {
                    EngineErrorDto::new(
                        ErrorCode::StreamError,
                        "The audio route stopped after its recovery budget was exhausted.",
                    )
                    .with_suggested_action(
                        "Reconnect the selected devices and start the route again.",
                    )
                })
            }),
        }
    }

    fn set_bypass(&mut self, bypassed: bool) -> Result<(), OperationFailure> {
        self.monitor
            .set_bypass(bypassed)
            .map_err(OperationFailure::from_headless)?;
        self.bypassed = bypassed;
        self.config.processing_mode = if bypassed {
            ProcessingMode::AlignedBypass
        } else {
            ProcessingMode::SpeechDenoise
        };
        Ok(())
    }

    fn update_settings(&mut self, settings: DenoiseSettings) -> Result<(), OperationFailure> {
        self.monitor
            .update_settings(settings.clone())
            .map_err(OperationFailure::from_headless)?;
        self.config.denoise = settings;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), OperationFailure> {
        let stopped = self.monitor.stop().map_err(OperationFailure::from_headless);
        self.endpoint_session.take();
        let released = platform_routing::current_router()
            .release_route(&self.config.id)
            .map_err(OperationFailure::from_routing);
        stopped.and(released)
    }
}

#[derive(Clone, Debug)]
struct OperationFailure {
    error: EngineErrorDto,
    recoverable: bool,
}

impl OperationFailure {
    fn new(code: ErrorCode, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            error: EngineErrorDto::new(code, message),
            recoverable,
        }
    }

    fn from_engine(error: EngineError) -> Self {
        Self {
            error: error.to_dto(),
            recoverable: false,
        }
    }

    fn from_audio_io(error: AudioIoError) -> Self {
        let (code, message, recoverable, action) = match error {
            AudioIoError::DeviceNotFound(_) | AudioIoError::NoDefaultDevice { .. } => (
                ErrorCode::DeviceNotFound,
                "The selected audio device is unavailable.",
                true,
                Some("Reconnect the device or choose another one."),
            ),
            AudioIoError::InvalidDeviceId(_) => (
                ErrorCode::InvalidIdentifier,
                "The selected device identifier is invalid.",
                false,
                None,
            ),
            AudioIoError::NoSupportedFormat { .. } => (
                ErrorCode::UnsupportedFormat,
                "The selected device has no supported PCM format.",
                false,
                Some("Choose another audio device."),
            ),
            AudioIoError::Backend {
                kind: AudioIoErrorKind::PermissionDenied,
                ..
            } => (
                ErrorCode::AudioIo,
                "Microphone access was denied by the operating system.",
                true,
                Some("Grant microphone permission, then refresh the device list."),
            ),
            AudioIoError::Backend { kind, .. } => (
                audio_kind_code(kind),
                "The operating system could not access the selected audio route.",
                audio_kind_recoverable(kind),
                Some("Check the selected devices and try again."),
            ),
            AudioIoError::CallbackBufferOverflow => (
                ErrorCode::AudioIo,
                "The negotiated callback buffer is too large.",
                false,
                Some("Choose another device or format."),
            ),
        };
        let mut failure = Self::new(code, message, recoverable);
        if let Some(action) = action {
            failure.error = failure.error.with_suggested_action(action);
        }
        failure
    }

    fn from_routing(error: platform_routing::RoutingError) -> Self {
        Self {
            error: error.to_dto(),
            recoverable: false,
        }
    }

    fn from_headless(error: HeadlessMonitorError) -> Self {
        match error {
            HeadlessMonitorError::Cancelled => Self::from_engine(EngineError::ShutdownInProgress),
            HeadlessMonitorError::WorkerCommand(crate::WorkerCommandError::QueueFull) => {
                Self::from_engine(EngineError::CommandQueueFull)
            }
            HeadlessMonitorError::AudioIo(error) => Self::from_audio_io(error),
            HeadlessMonitorError::Routing(error) => Self::from_routing(error),
            HeadlessMonitorError::Denoise(_) => Self::new(
                ErrorCode::Denoise,
                "The noise-reduction model could not start or process audio.",
                false,
            ),
            HeadlessMonitorError::State(_) => Self::new(
                ErrorCode::InvalidState,
                "The audio route received an invalid lifecycle transition.",
                false,
            ),
            HeadlessMonitorError::InvalidConfiguration(_) => Self::new(
                ErrorCode::InvalidConfiguration,
                "The audio route configuration is invalid.",
                false,
            ),
            HeadlessMonitorError::StreamFault { kind, .. } => Self::new(
                audio_kind_code(kind),
                "An audio stream stopped unexpectedly.",
                audio_kind_recoverable(kind),
            ),
            HeadlessMonitorError::ActivationTimeout { .. }
            | HeadlessMonitorError::WorkerStoppedDuringActivation
            | HeadlessMonitorError::RecoveryExhausted { .. } => Self::new(
                ErrorCode::StreamError,
                "The audio route could not become stable within its recovery budget.",
                true,
            ),
            HeadlessMonitorError::Worker(_) | HeadlessMonitorError::WorkerCommand(_) => Self::new(
                ErrorCode::WorkerTerminated,
                "The audio processing worker stopped unexpectedly.",
                false,
            ),
            HeadlessMonitorError::WorkerStart(_) => Self::new(
                ErrorCode::WorkerTerminated,
                "The audio processing worker could not start.",
                false,
            ),
            HeadlessMonitorError::Transport(_) | HeadlessMonitorError::QueueSizeOverflow => {
                Self::new(
                    ErrorCode::Dsp,
                    "The bounded audio transport could not be configured.",
                    false,
                )
            }
        }
    }
}

fn run_supervisor(
    config: AudioEngineConfig,
    commands: Receiver<EngineCommand>,
    events: SyncSender<EngineObservation>,
    snapshot: EngineSnapshotStore,
    shutdown_requested: Arc<AtomicBool>,
    mut driver: Box<dyn EngineDriver>,
) {
    let mut active_route: Option<Box<dyn ManagedRoute>> = None;
    refresh_devices(&mut *driver, &snapshot, &events);
    let mut last_metrics_at = Instant::now();

    while !shutdown_requested.load(Ordering::Acquire) {
        match commands.recv_timeout(config.poll_interval) {
            Ok(EngineCommand::Shutdown) => shutdown_requested.store(true, Ordering::Release),
            Ok(command) => process_command(
                command,
                &mut *driver,
                &mut active_route,
                &snapshot,
                &events,
                &shutdown_requested,
            ),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                shutdown_requested.store(true, Ordering::Release);
            }
        }

        if last_metrics_at.elapsed() >= config.metrics_interval {
            poll_active_route(&mut active_route, &snapshot, &events);
            last_metrics_at = Instant::now();
        }
    }

    snapshot.update(|value| value.shutting_down = true);
    if let Some(mut route) = active_route.take() {
        let route_id = route.route_id().clone();
        let mut stopping = route.snapshot();
        stopping.state = PipelineState::Stopping;
        publish_route(&snapshot, stopping);
        emit(
            &snapshot,
            &events,
            EngineEvent::StateChanged {
                route_id: route_id.clone(),
                state: PipelineState::Stopping,
            },
        );
        let stop_result = route.shutdown();
        let mut stopped = route.snapshot();
        stopped.state = PipelineState::Stopped;
        if let Err(failure) = stop_result {
            stopped.fault = Some(failure.error.clone());
            record_fault(&snapshot, &events, Some(route_id.clone()), failure);
        }
        publish_route(&snapshot, stopped);
        emit(
            &snapshot,
            &events,
            EngineEvent::StateChanged {
                route_id,
                state: PipelineState::Stopped,
            },
        );
    }
}

fn process_command(
    command: EngineCommand,
    driver: &mut dyn EngineDriver,
    active_route: &mut Option<Box<dyn ManagedRoute>>,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
    shutdown_requested: &AtomicBool,
) {
    match command {
        EngineCommand::RefreshDevices => refresh_devices(driver, snapshot, events),
        EngineCommand::StartRoute(config) => start_route(
            config,
            driver,
            active_route,
            snapshot,
            events,
            shutdown_requested,
        ),
        EngineCommand::StopRoute(route_id) => {
            stop_route(route_id, active_route, snapshot, events);
        }
        EngineCommand::UpdateDenoise { route_id, settings } => {
            update_denoise(route_id, settings, active_route, snapshot, events);
        }
        EngineCommand::SetBypass { route_id, bypassed } => {
            set_bypass(route_id, bypassed, active_route, snapshot, events)
        }
        EngineCommand::Shutdown => shutdown_requested.store(true, Ordering::Release),
    }
}

fn refresh_devices(
    driver: &mut dyn EngineDriver,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
) {
    match driver.inventory() {
        Ok(inventory) => {
            snapshot.update(|value| {
                value.routing = platform_routing::current_router().snapshot(&inventory);
                value.inventory = inventory;
            });
            emit(snapshot, events, EngineEvent::DevicesChanged);
        }
        Err(failure) => record_fault(snapshot, events, None, failure),
    }
}

fn start_route(
    config: RouteConfig,
    driver: &mut dyn EngineDriver,
    active_route: &mut Option<Box<dyn ManagedRoute>>,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
    shutdown_requested: &AtomicBool,
) {
    if let Err(error) = validate_route_config(&config) {
        record_fault(
            snapshot,
            events,
            Some(config.id.clone()),
            OperationFailure::from_engine(error),
        );
        return;
    }

    if let Some(route) = active_route.as_mut() {
        let current = route.snapshot();
        if current.state == PipelineState::Failed {
            let _ = route.stop();
            *active_route = None;
        } else {
            record_fault(
                snapshot,
                events,
                Some(config.id.clone()),
                OperationFailure::from_engine(EngineError::InvalidState {
                    route_id: config.id,
                    state: current.state,
                }),
            );
            return;
        }
    }

    let route_id = config.id.clone();
    snapshot.update(|value| {
        value.routes.clear();
        value.last_fault = None;
    });
    publish_route(
        snapshot,
        RouteSnapshot::pending(config.clone(), PipelineState::Starting),
    );
    emit(
        snapshot,
        events,
        EngineEvent::StateChanged {
            route_id: route_id.clone(),
            state: PipelineState::Starting,
        },
    );

    match driver.start_route(config.clone()) {
        Ok(mut route) => {
            if shutdown_requested.load(Ordering::Acquire) {
                let _ = route.shutdown();
                let mut stopped = route.snapshot();
                stopped.state = PipelineState::Stopped;
                publish_route(snapshot, stopped);
                return;
            }
            let running = route.snapshot();
            let state = running.state;
            publish_route(snapshot, running);
            emit(
                snapshot,
                events,
                EngineEvent::StateChanged { route_id, state },
            );
            *active_route = Some(route);
        }
        Err(failure) => {
            let mut failed = RouteSnapshot::pending(config, PipelineState::Failed);
            failed.fault = Some(failure.error.clone());
            publish_route(snapshot, failed);
            emit(
                snapshot,
                events,
                EngineEvent::StateChanged {
                    route_id: route_id.clone(),
                    state: PipelineState::Failed,
                },
            );
            record_fault(snapshot, events, Some(route_id), failure);
        }
    }
}

fn stop_route(
    route_id: RouteId,
    active_route: &mut Option<Box<dyn ManagedRoute>>,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
) {
    let Some(route) = active_route.as_mut() else {
        let mut existing = snapshot
            .snapshot()
            .routes
            .into_iter()
            .find(|route| route.config.id == route_id);
        match existing.as_mut() {
            Some(route) if route.state == PipelineState::Failed => {
                route.state = PipelineState::Stopping;
                publish_route(snapshot, route.clone());
                emit(
                    snapshot,
                    events,
                    EngineEvent::StateChanged {
                        route_id: route_id.clone(),
                        state: PipelineState::Stopping,
                    },
                );
                route.state = PipelineState::Stopped;
                publish_route(snapshot, route.clone());
                emit(
                    snapshot,
                    events,
                    EngineEvent::StateChanged {
                        route_id,
                        state: PipelineState::Stopped,
                    },
                );
            }
            Some(route) if route.state == PipelineState::Stopped => {}
            _ => record_fault(
                snapshot,
                events,
                Some(route_id.clone()),
                OperationFailure::from_engine(EngineError::RouteNotFound(route_id)),
            ),
        }
        return;
    };
    if route.route_id() != &route_id {
        record_fault(
            snapshot,
            events,
            Some(route_id.clone()),
            OperationFailure::from_engine(EngineError::RouteNotFound(route_id)),
        );
        return;
    }

    let mut stopping = route.snapshot();
    stopping.state = PipelineState::Stopping;
    publish_route(snapshot, stopping);
    emit(
        snapshot,
        events,
        EngineEvent::StateChanged {
            route_id: route_id.clone(),
            state: PipelineState::Stopping,
        },
    );
    let stop_result = route.stop();
    let mut stopped = route.snapshot();
    let state = stopped.state;
    if let Err(failure) = stop_result {
        stopped.fault = Some(failure.error.clone());
        record_fault(snapshot, events, Some(route_id.clone()), failure);
    }
    publish_route(snapshot, stopped);
    emit(
        snapshot,
        events,
        EngineEvent::StateChanged { route_id, state },
    );
    if state == PipelineState::Stopped {
        *active_route = None;
    }
}

fn update_denoise(
    route_id: RouteId,
    settings: DenoiseSettings,
    active_route: &mut Option<Box<dyn ManagedRoute>>,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
) {
    let Some(route) = matching_running_route(route_id, active_route, snapshot, events) else {
        return;
    };
    match route.update_settings(settings) {
        Ok(()) => publish_route(snapshot, route.snapshot()),
        Err(failure) => record_fault(snapshot, events, Some(route.route_id().clone()), failure),
    }
}

fn set_bypass(
    route_id: RouteId,
    bypassed: bool,
    active_route: &mut Option<Box<dyn ManagedRoute>>,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
) {
    let Some(route) = matching_running_route(route_id, active_route, snapshot, events) else {
        return;
    };
    match route.set_bypass(bypassed) {
        Ok(()) => publish_route(snapshot, route.snapshot()),
        Err(failure) => record_fault(snapshot, events, Some(route.route_id().clone()), failure),
    }
}

fn matching_running_route<'a>(
    route_id: RouteId,
    active_route: &'a mut Option<Box<dyn ManagedRoute>>,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
) -> Option<&'a mut Box<dyn ManagedRoute>> {
    let Some(route) = active_route.as_mut() else {
        record_fault(
            snapshot,
            events,
            Some(route_id.clone()),
            OperationFailure::from_engine(EngineError::RouteNotFound(route_id)),
        );
        return None;
    };
    let current = route.snapshot();
    if route.route_id() != &route_id || current.state != PipelineState::Running {
        record_fault(
            snapshot,
            events,
            Some(route_id.clone()),
            OperationFailure::from_engine(EngineError::InvalidState {
                route_id,
                state: current.state,
            }),
        );
        return None;
    }
    Some(route)
}

fn poll_active_route(
    active_route: &mut Option<Box<dyn ManagedRoute>>,
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
) {
    let Some(route) = active_route.as_mut() else {
        return;
    };
    let previous = snapshot
        .snapshot()
        .routes
        .into_iter()
        .find(|candidate| candidate.config.id == *route.route_id());
    let current = route.snapshot();
    publish_route(snapshot, current.clone());
    if previous
        .as_ref()
        .is_none_or(|value| value.state != current.state)
    {
        emit(
            snapshot,
            events,
            EngineEvent::StateChanged {
                route_id: current.config.id.clone(),
                state: current.state,
            },
        );
    }
    if let Some(error) = current.fault.clone()
        && previous
            .as_ref()
            .is_none_or(|value| value.fault.as_ref() != Some(&error))
    {
        record_fault(
            snapshot,
            events,
            Some(current.config.id.clone()),
            OperationFailure {
                error,
                recoverable: true,
            },
        );
    }
    emit(
        snapshot,
        events,
        EngineEvent::Metrics {
            route_id: current.config.id.clone(),
            metrics: current.metrics.clone(),
        },
    );
    let stopped = current.state == PipelineState::Stopped;
    if stopped {
        *active_route = None;
    }
}

fn publish_route(snapshot: &EngineSnapshotStore, mut route: RouteSnapshot) {
    snapshot.update(|value| {
        // Failures remain visible until an explicit route restart. Periodic successful metrics
        // are not an acknowledgement of a failed command.
        if route.fault.is_none() {
            route.fault = value
                .routes
                .iter()
                .find(|candidate| candidate.config.id == route.config.id)
                .and_then(|candidate| candidate.fault.clone());
        }
        if route.fault.is_some() {
            value.last_fault = route.fault.clone();
        }
        // Phase 4 supports exactly one route; retain only its latest active/terminal observation.
        value.routes.clear();
        value.routes.push(route);
    });
}

fn record_fault(
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
    route_id: Option<RouteId>,
    failure: OperationFailure,
) {
    snapshot.update(|value| {
        value.last_fault = Some(failure.error.clone());
        if let Some(route_id) = route_id.as_ref()
            && let Some(route) = value
                .routes
                .iter_mut()
                .find(|route| &route.config.id == route_id)
        {
            route.fault = Some(failure.error.clone());
        }
    });
    emit(
        snapshot,
        events,
        EngineEvent::Fault {
            route_id,
            error: failure.error,
            recoverable: failure.recoverable,
        },
    );
}

fn emit(
    snapshot: &EngineSnapshotStore,
    events: &SyncSender<EngineObservation>,
    event: EngineEvent,
) {
    // The snapshot remains authoritative if the UI is slow. Dropping a redundant low-rate event
    // is preferable to blocking the supervisor or creating an unbounded queue.
    let _ = events.try_send(EngineObservation {
        revision: snapshot.snapshot().revision,
        event,
    });
}

fn validate_route_config(config: &RouteConfig) -> Result<(), EngineError> {
    config
        .validate()
        .map_err(|_| EngineError::InvalidConfiguration("invalid denoise settings"))?;
    if config.target_latency_ms.get() > MAX_TARGET_LATENCY_MS {
        return Err(EngineError::InvalidConfiguration(
            "target latency must not exceed 1000 milliseconds",
        ));
    }
    if !matches!(config.source, SourceEndpoint::PhysicalInput(_)) {
        return Err(EngineError::InvalidConfiguration(
            "only physical microphone sources are supported in this release",
        ));
    }
    if !matches!(
        config.destination,
        DestinationEndpoint::PhysicalOutput(_) | DestinationEndpoint::VirtualMicrophone(_)
    ) {
        return Err(EngineError::InvalidConfiguration(
            "only physical outputs and supported virtual microphones are available",
        ));
    }
    Ok(())
}

fn stream_diagnostics(
    callbacks: CallbackMetricsSnapshot,
    issues: StreamIssueSnapshot,
) -> StreamDiagnostics {
    StreamDiagnostics {
        callbacks: callbacks.callbacks,
        frames: callbacks.frames,
        issues: issues.total,
        terminal_issues: issues.terminal,
        xruns: issues.xruns,
        callback_violations: issues.callback_violations,
        last_issue: issues.last_kind.map(issue_label).map(str::to_owned),
        last_terminal_issue: issues
            .last_terminal_kind
            .map(issue_label)
            .map(str::to_owned),
    }
}

fn issue_label(kind: StreamIssueKind) -> &'static str {
    match kind {
        AudioIoErrorKind::DeviceBusy => "device_busy",
        AudioIoErrorKind::DeviceChanged => "device_changed",
        AudioIoErrorKind::DeviceNotAvailable => "device_not_available",
        AudioIoErrorKind::HostUnavailable => "host_unavailable",
        AudioIoErrorKind::InvalidInput => "invalid_input",
        AudioIoErrorKind::PermissionDenied => "permission_denied",
        AudioIoErrorKind::RealtimeDenied => "realtime_denied",
        AudioIoErrorKind::ResourceExhausted => "resource_exhausted",
        AudioIoErrorKind::StreamInvalidated => "stream_invalidated",
        AudioIoErrorKind::UnsupportedConfig => "unsupported_config",
        AudioIoErrorKind::UnsupportedOperation => "unsupported_operation",
        AudioIoErrorKind::Xrun => "xrun",
        AudioIoErrorKind::Backend => "backend",
        AudioIoErrorKind::Other => "other",
        _ => "other",
    }
}

fn audio_kind_code(kind: AudioIoErrorKind) -> ErrorCode {
    match kind {
        AudioIoErrorKind::DeviceChanged
        | AudioIoErrorKind::DeviceNotAvailable
        | AudioIoErrorKind::StreamInvalidated => ErrorCode::DeviceDisconnected,
        AudioIoErrorKind::UnsupportedConfig | AudioIoErrorKind::UnsupportedOperation => {
            ErrorCode::UnsupportedFormat
        }
        _ => ErrorCode::AudioIo,
    }
}

fn audio_kind_recoverable(kind: AudioIoErrorKind) -> bool {
    !matches!(
        kind,
        AudioIoErrorKind::InvalidInput
            | AudioIoErrorKind::UnsupportedConfig
            | AudioIoErrorKind::UnsupportedOperation
    )
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU16,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc::{Sender, channel},
        },
    };

    use audio_protocol::{DeviceId, ProcessingMode};

    use super::*;

    struct FakeDriver {
        start_gate: Option<Receiver<()>>,
        stops: Arc<AtomicUsize>,
    }

    impl EngineDriver for FakeDriver {
        fn inventory(&mut self) -> Result<DeviceInventory, OperationFailure> {
            Ok(DeviceInventory {
                host: "fake".to_owned(),
                devices: Vec::new(),
            })
        }

        fn start_route(
            &mut self,
            config: RouteConfig,
        ) -> Result<Box<dyn ManagedRoute>, OperationFailure> {
            if let Some(gate) = self.start_gate.take() {
                let _ = gate.recv();
            }
            Ok(Box::new(FakeRoute {
                route_id: config.id.clone(),
                snapshot: Mutex::new(RouteSnapshot::pending(config, PipelineState::Running)),
                stops: self.stops.clone(),
            }))
        }
    }

    pub(super) struct FakeRoute {
        pub(super) route_id: RouteId,
        pub(super) snapshot: Mutex<RouteSnapshot>,
        pub(super) stops: Arc<AtomicUsize>,
    }

    impl ManagedRoute for FakeRoute {
        fn route_id(&self) -> &RouteId {
            &self.route_id
        }

        fn snapshot(&mut self) -> RouteSnapshot {
            match self.snapshot.lock() {
                Ok(snapshot) => snapshot.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            }
        }

        fn set_bypass(&mut self, bypassed: bool) -> Result<(), OperationFailure> {
            let mut snapshot = match self.snapshot.lock() {
                Ok(snapshot) => snapshot,
                Err(poisoned) => poisoned.into_inner(),
            };
            snapshot.bypassed = bypassed;
            snapshot.config.processing_mode = if bypassed {
                ProcessingMode::AlignedBypass
            } else {
                ProcessingMode::SpeechDenoise
            };
            Ok(())
        }

        fn update_settings(&mut self, settings: DenoiseSettings) -> Result<(), OperationFailure> {
            match self.snapshot.lock() {
                Ok(mut snapshot) => snapshot.config.denoise = settings,
                Err(poisoned) => poisoned.into_inner().config.denoise = settings,
            }
            Ok(())
        }

        fn stop(&mut self) -> Result<(), OperationFailure> {
            self.stops.fetch_add(1, Ordering::Relaxed);
            match self.snapshot.lock() {
                Ok(mut snapshot) => snapshot.state = PipelineState::Stopped,
                Err(poisoned) => poisoned.into_inner().state = PipelineState::Stopped,
            }
            Ok(())
        }
    }

    pub(super) fn route_config() -> RouteConfig {
        RouteConfig {
            id: RouteId::new("desktop-monitor").expect("valid route ID"),
            source: SourceEndpoint::PhysicalInput(
                DeviceId::new("fake:input").expect("valid input ID"),
            ),
            destination: DestinationEndpoint::PhysicalOutput(
                DeviceId::new("fake:output").expect("valid output ID"),
            ),
            processing_mode: ProcessingMode::SpeechDenoise,
            target_latency_ms: NonZeroU16::new(40).expect("non-zero latency"),
            denoise: DenoiseSettings::default(),
        }
    }

    struct CancellableDriver {
        entered: SyncSender<()>,
        stops: Arc<AtomicUsize>,
    }

    impl EngineDriver for CancellableDriver {
        fn inventory(&mut self) -> Result<DeviceInventory, OperationFailure> {
            Ok(DeviceInventory::default())
        }
        fn start_route(
            &mut self,
            config: RouteConfig,
        ) -> Result<Box<dyn ManagedRoute>, OperationFailure> {
            let entered = self.entered.clone();
            let stops = self.stops.clone();
            Ok(Box::new(OwnedRoute::spawn(
                config.clone(),
                move |control| {
                    entered.send(()).expect("startup entered");
                    let _ = control.wait(Duration::from_secs(30));
                    // Even a backend that completes successfully after cancellation must be cleaned
                    // up on its owner, without publishing Running or accepting more PCM.
                    Ok(Box::new(FakeRoute {
                        route_id: config.id.clone(),
                        snapshot: Mutex::new(RouteSnapshot::pending(
                            config,
                            PipelineState::Running,
                        )),
                        stops,
                    }))
                },
            )?))
        }
    }

    #[test]
    fn stop_and_shutdown_interrupt_startup_without_publishing_running() {
        for shutdown in [false, true] {
            let (entered, receiver) = sync_channel(1);
            let stops = Arc::new(AtomicUsize::new(0));
            let driver_stops = stops.clone();
            let mut engine =
                AudioEngine::start_with_driver(AudioEngineConfig::default(), move || {
                    Box::new(CancellableDriver {
                        entered,
                        stops: driver_stops,
                    })
                })
                .expect("engine starts");
            let events = engine.take_event_receiver().expect("events");
            let handle = engine.handle();
            handle
                .try_send(EngineCommand::StartRoute(route_config()))
                .expect("start");
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("startup in progress");
            if shutdown {
                handle.request_shutdown();
            } else {
                handle
                    .try_send(EngineCommand::StopRoute(route_config().id))
                    .expect("stop");
            }
            wait_for(&handle, |snapshot| {
                snapshot
                    .routes
                    .first()
                    .is_some_and(|route| route.state == PipelineState::Stopped)
            });
            engine.shutdown().expect("joined");
            assert_eq!(stops.load(Ordering::Relaxed), 1);
            assert!(!events.try_iter().any(|event| matches!(
                event.event,
                EngineEvent::StateChanged {
                    state: PipelineState::Running,
                    ..
                }
            )));
        }
    }

    #[test]
    fn polling_publishes_before_events_and_keeps_faults_when_events_are_dropped() {
        let store = EngineSnapshotStore::default();
        let (events, receiver) = sync_channel(1);
        let mut driver = FakeDriver {
            start_gate: None,
            stops: Arc::new(AtomicUsize::new(0)),
        };
        let mut active = Some(driver.start_route(route_config()).expect("route"));
        poll_active_route(&mut active, &store, &events);
        let event = receiver.recv().expect("state event");
        assert!(event.revision > 0);
        assert_eq!(event.revision, store.snapshot().revision);
        assert_eq!(store.snapshot().routes[0].state, PipelineState::Running);
        // Fill the channel so every subsequent fault/metrics event must be dropped.
        emit(&store, &events, EngineEvent::DevicesChanged);
        record_fault(
            &store,
            &events,
            Some(route_config().id),
            OperationFailure::from_engine(EngineError::CommandQueueFull),
        );
        for _ in 0..10 {
            poll_active_route(&mut active, &store, &events);
        }
        assert_eq!(
            store
                .snapshot()
                .last_fault
                .as_ref()
                .expect("latched fault")
                .code,
            ErrorCode::CommandQueueFull
        );
        assert!(store.snapshot().routes[0].fault.is_some());
        assert_eq!(receiver.try_iter().count(), 1);
    }

    #[test]
    fn terminal_route_retention_is_bounded() {
        let store = EngineSnapshotStore::default();
        for index in 0..2_000 {
            let mut config = route_config();
            config.id = RouteId::new(format!("route-{index}")).expect("ID");
            publish_route(
                &store,
                RouteSnapshot::pending(config, PipelineState::Stopped),
            );
            assert_eq!(store.snapshot().routes.len(), 1);
        }
        assert_eq!(store.snapshot().routes[0].config.id.as_str(), "route-1999");
    }

    fn test_engine(
        config: AudioEngineConfig,
        gate: Option<Receiver<()>>,
        stops: Arc<AtomicUsize>,
    ) -> AudioEngine {
        AudioEngine::start_with_driver(config, move || {
            Box::new(FakeDriver {
                start_gate: gate,
                stops,
            })
        })
        .expect("fake engine starts")
    }

    fn wait_for(
        handle: &AudioEngineHandle,
        predicate: impl Fn(&EngineSnapshot) -> bool,
    ) -> EngineSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = handle.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "snapshot condition timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn bounded_command_queue_reports_backpressure() {
        let (release_tx, release_rx): (Sender<()>, Receiver<()>) = channel();
        let stops = Arc::new(AtomicUsize::new(0));
        let mut engine = test_engine(
            AudioEngineConfig {
                command_capacity: 1,
                ..AudioEngineConfig::default()
            },
            Some(release_rx),
            stops,
        );
        let handle = engine.handle();
        handle
            .try_send(EngineCommand::StartRoute(route_config()))
            .expect("start command accepted");
        wait_for(&handle, |snapshot| {
            snapshot
                .routes
                .first()
                .is_some_and(|route| route.state == PipelineState::Starting)
        });

        handle
            .try_send(EngineCommand::RefreshDevices)
            .expect("first queued command accepted");
        assert_eq!(
            handle.try_send(EngineCommand::RefreshDevices),
            Err(EngineError::CommandQueueFull)
        );
        release_tx.send(()).expect("release fake startup");
        engine.shutdown().expect("engine shuts down");
    }

    #[test]
    fn snapshot_reconciles_start_updates_bypass_and_stop() {
        let stops = Arc::new(AtomicUsize::new(0));
        let mut engine = test_engine(AudioEngineConfig::default(), None, stops.clone());
        let handle = engine.handle();
        handle
            .try_send(EngineCommand::StartRoute(route_config()))
            .expect("start command accepted");
        wait_for(&handle, |snapshot| {
            snapshot
                .routes
                .first()
                .is_some_and(|route| route.state == PipelineState::Running)
        });

        handle
            .try_send(EngineCommand::SetBypass {
                route_id: route_config().id,
                bypassed: true,
            })
            .expect("bypass command accepted");
        let settings = DenoiseSettings {
            attenuation_limit_db: Some(24.0),
            ..DenoiseSettings::default()
        };
        handle
            .try_send(EngineCommand::UpdateDenoise {
                route_id: route_config().id,
                settings: settings.clone(),
            })
            .expect("settings command accepted");
        let snapshot = wait_for(&handle, |snapshot| {
            snapshot
                .routes
                .first()
                .is_some_and(|route| route.bypassed && route.config.denoise == settings)
        });
        assert!(snapshot.revision >= 4);

        handle
            .try_send(EngineCommand::StopRoute(route_config().id))
            .expect("stop command accepted");
        wait_for(&handle, |snapshot| {
            snapshot
                .routes
                .first()
                .is_some_and(|route| route.state == PipelineState::Stopped)
        });
        assert_eq!(stops.load(Ordering::Relaxed), 1);
        engine.shutdown().expect("engine shuts down");
        assert!(handle.snapshot().shutting_down);
    }

    #[test]
    fn shutdown_signal_stops_active_route_without_using_the_command_queue() {
        let stops = Arc::new(AtomicUsize::new(0));
        let mut engine = test_engine(AudioEngineConfig::default(), None, stops.clone());
        let handle = engine.handle();
        handle
            .try_send(EngineCommand::StartRoute(route_config()))
            .expect("start command accepted");
        wait_for(&handle, |snapshot| {
            snapshot
                .routes
                .first()
                .is_some_and(|route| route.state == PipelineState::Running)
        });

        handle.request_shutdown();
        engine.shutdown().expect("engine shuts down");
        engine.shutdown().expect("repeated shutdown is harmless");
        assert_eq!(stops.load(Ordering::Relaxed), 1);
        let snapshot = handle.snapshot();
        assert!(snapshot.shutting_down);
        assert_eq!(snapshot.routes[0].state, PipelineState::Stopped);
        assert_eq!(
            handle.try_send(EngineCommand::RefreshDevices),
            Err(EngineError::ShutdownInProgress)
        );
    }

    #[test]
    fn failed_route_without_live_resources_can_be_stopped() {
        let snapshot = EngineSnapshotStore::default();
        let (events, receiver) = sync_channel(4);
        let mut failed = RouteSnapshot::pending(route_config(), PipelineState::Failed);
        failed.fault = Some(EngineErrorDto::new(ErrorCode::StreamError, "route failed"));
        publish_route(&snapshot, failed);

        stop_route(route_config().id, &mut None, &snapshot, &events);

        assert_eq!(snapshot.snapshot().routes[0].state, PipelineState::Stopped);
        assert!(matches!(
            receiver.recv().expect("stopping event").event,
            EngineEvent::StateChanged {
                state: PipelineState::Stopping,
                ..
            }
        ));
        assert!(matches!(
            receiver.recv().expect("stopped event").event,
            EngineEvent::StateChanged {
                state: PipelineState::Stopped,
                ..
            }
        ));
    }

    #[test]
    fn stopping_failed_route_releases_its_retained_owner_once() {
        let snapshot = EngineSnapshotStore::default();
        let (events, _receiver) = sync_channel(8);
        let stops = Arc::new(AtomicUsize::new(0));
        let failed = RouteSnapshot::pending(route_config(), PipelineState::Failed);
        publish_route(&snapshot, failed.clone());
        let mut active: Option<Box<dyn ManagedRoute>> = Some(Box::new(FakeRoute {
            route_id: failed.config.id.clone(),
            snapshot: Mutex::new(failed),
            stops: stops.clone(),
        }));
        stop_route(route_config().id, &mut active, &snapshot, &events);
        assert!(active.is_none());
        assert_eq!(snapshot.snapshot().routes[0].state, PipelineState::Stopped);
        assert_eq!(stops.load(Ordering::Relaxed), 1);
        stop_route(route_config().id, &mut active, &snapshot, &events);
        assert_eq!(stops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn invalid_settings_are_rejected_before_enqueue() {
        let stops = Arc::new(AtomicUsize::new(0));
        let mut engine = test_engine(AudioEngineConfig::default(), None, stops);
        let handle = engine.handle();
        let error = handle.try_send(EngineCommand::UpdateDenoise {
            route_id: route_config().id,
            settings: DenoiseSettings {
                attenuation_limit_db: Some(f32::NAN),
                ..DenoiseSettings::default()
            },
        });
        assert_eq!(
            error,
            Err(EngineError::InvalidConfiguration(
                "invalid denoise settings"
            ))
        );
        engine.shutdown().expect("engine shuts down");
    }

    #[test]
    fn virtual_destination_survives_supervision_restart_and_shutdown() {
        let stops = Arc::new(AtomicUsize::new(0));
        let mut engine = test_engine(AudioEngineConfig::default(), None, stops.clone());
        let handle = engine.handle();
        let mut config = route_config();
        config.destination = DestinationEndpoint::VirtualMicrophone(
            audio_protocol::VirtualEndpointId::new("macos:blackhole-2ch").expect("ID"),
        );
        for expected_stops in 1..=3 {
            handle
                .try_send(EngineCommand::StartRoute(config.clone()))
                .expect("virtual start accepted");
            let snapshot = wait_for(&handle, |snapshot| {
                snapshot
                    .routes
                    .first()
                    .is_some_and(|route| route.state == PipelineState::Running)
            });
            assert_eq!(snapshot.routes[0].config.destination, config.destination);
            handle
                .try_send(EngineCommand::StopRoute(config.id.clone()))
                .expect("stop accepted");
            wait_for(&handle, |snapshot| {
                snapshot.routes[0].state == PipelineState::Stopped
            });
            assert_eq!(stops.load(Ordering::Relaxed), expected_stops);
        }
        handle
            .try_send(EngineCommand::StartRoute(config))
            .expect("restart");
        wait_for(&handle, |snapshot| {
            snapshot.routes[0].state == PipelineState::Running
        });
        engine.shutdown().expect("shutdown joins virtual route");
        assert_eq!(stops.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn rejected_platform_preparation_publishes_actionable_fault_and_can_stop() {
        let config = route_config();
        let mut owned = OwnedRoute::spawn(config, |_| {
            Err(OperationFailure::from_routing(
                platform_routing::RoutingError::MissingEndpoint,
            ))
        })
        .expect("owner starts");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = owned.snapshot();
            if snapshot.state == PipelineState::Failed {
                let fault = snapshot.fault.expect("fault");
                assert_eq!(fault.code, ErrorCode::Routing);
                assert!(
                    fault
                        .suggested_action
                        .expect("instructions")
                        .contains("setup instructions")
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owner must report preparation failure"
            );
            thread::sleep(Duration::from_millis(2));
        }
        owned.shutdown().expect("failed owner joined");
        assert_eq!(owned.snapshot().state, PipelineState::Stopped);
    }
}
