use std::{
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use audio_cpal::{
    AudioIoError, CallbackMetricsSnapshot, CpalHost, CpalStream, DeviceSelection,
    NegotiationPreferences, StreamDirection, StreamIssueKind, StreamIssueSnapshot,
    StreamIssueStore,
};
use audio_protocol::{
    AudioDeviceDescriptor, AudioFormat, ChannelCount, DenoiseSettings, PipelineMetrics,
    PipelineState, RouteId, SampleRateHz,
};
use denoise_core::{DenoiseError, NoiseReducerDescriptor};
use denoise_dfn::{DfnNoiseReducer, ModelAsset, default_model_path};

use crate::{
    AudioRingProducer, AudioTransportError, PipelineWorkerConfig, PipelineWorkerHandle,
    PipelineWorkerWake, RecoveryAction, RecoveryBudget, RouteMetricsStore, RouteRecoveryPolicy,
    RouteStateMachine, StateTransitionError, WorkerCommandError, WorkerError, WorkerStartError,
    audio_error_action, bounded_audio_ring, stream_issue_action,
};

use crate::{lifecycle::MonitorControl, worker::spawn_pipeline_worker_controlled};

const MODEL_WARM_UP_FRAMES: usize = 3;

/// Physical monitor startup options with bounded queue durations.
#[derive(Clone, Debug)]
pub struct HeadlessMonitorConfig {
    pub route_id: RouteId,
    pub input: DeviceSelection,
    pub output: DeviceSelection,
    pub model_path: PathBuf,
    pub denoise: DenoiseSettings,
    pub initially_bypassed: bool,
    pub target_latency_ms: u16,
    pub input_capacity_ms: u16,
    pub output_capacity_ms: u16,
    pub input_stale_target_ms: u16,
    pub crossfade_ms: u16,
    pub recovery: RouteRecoveryPolicy,
}

impl Default for HeadlessMonitorConfig {
    fn default() -> Self {
        let route_id = match RouteId::new("headless-monitor") {
            Ok(route_id) => route_id,
            Err(_) => unreachable!("static route identifier is valid"),
        };
        Self {
            route_id,
            input: DeviceSelection::Default,
            output: DeviceSelection::Default,
            model_path: default_model_path(),
            denoise: DenoiseSettings::default(),
            initially_bypassed: false,
            target_latency_ms: 40,
            input_capacity_ms: 100,
            output_capacity_ms: 200,
            input_stale_target_ms: 80,
            crossfade_ms: 20,
            recovery: RouteRecoveryPolicy::default(),
        }
    }
}

impl HeadlessMonitorConfig {
    fn validate(&self) -> Result<(), HeadlessMonitorError> {
        if self.target_latency_ms == 0
            || self.input_capacity_ms == 0
            || self.output_capacity_ms == 0
            || self.input_stale_target_ms == 0
        {
            return Err(HeadlessMonitorError::InvalidConfiguration(
                "latency and queue durations must be non-zero",
            ));
        }
        if self.target_latency_ms > self.output_capacity_ms {
            return Err(HeadlessMonitorError::InvalidConfiguration(
                "target latency exceeds output capacity",
            ));
        }
        if self.input_stale_target_ms > self.input_capacity_ms {
            return Err(HeadlessMonitorError::InvalidConfiguration(
                "input stale target exceeds input capacity",
            ));
        }
        self.denoise
            .validate()
            .map_err(|_| HeadlessMonitorError::InvalidConfiguration("invalid denoise settings"))?;
        self.recovery
            .validate()
            .map_err(HeadlessMonitorError::InvalidConfiguration)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HeadlessMonitorError {
    #[error("route startup or recovery was cancelled")]
    Cancelled,
    #[error(transparent)]
    AudioIo(#[from] AudioIoError),
    #[error(transparent)]
    Denoise(#[from] DenoiseError),
    #[error(transparent)]
    Transport(#[from] AudioTransportError),
    #[error(transparent)]
    WorkerStart(#[from] WorkerStartError),
    #[error(transparent)]
    Worker(#[from] WorkerError),
    #[error(transparent)]
    WorkerCommand(#[from] WorkerCommandError),
    #[error(transparent)]
    State(#[from] StateTransitionError),
    #[error("invalid headless monitor configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("audio queue frame count overflowed addressable memory")]
    QueueSizeOverflow,
    #[error("{direction:?} stream activation did not produce callbacks before timeout")]
    ActivationTimeout { direction: StreamDirection },
    #[error("{direction:?} stream requires supervisor action after {kind:?}")]
    StreamFault {
        direction: StreamDirection,
        kind: StreamIssueKind,
    },
    #[error("DSP worker stopped while a route generation was activating")]
    WorkerStoppedDuringActivation,
    #[error("route recovery exhausted after {attempts} attempts: {last_error}")]
    RecoveryExhausted { attempts: u8, last_error: String },
}

/// Current headless route state and low-rate diagnostics.
#[derive(Clone, Debug)]
pub struct HeadlessMonitorSnapshot {
    pub route_id: RouteId,
    pub state: PipelineState,
    pub input_device: AudioDeviceDescriptor,
    pub output_device: AudioDeviceDescriptor,
    pub input_format: AudioFormat,
    pub output_format: AudioFormat,
    pub model: NoiseReducerDescriptor,
    pub metrics: PipelineMetrics,
    pub model_errors: u64,
    pub input_issues: StreamIssueSnapshot,
    pub output_issues: StreamIssueSnapshot,
    pub input_callbacks: CallbackMetricsSnapshot,
    pub output_callbacks: CallbackMetricsSnapshot,
    pub recovery_attempts: u64,
    pub successful_recoveries: u64,
    pub last_recovery_error: Option<String>,
    pub last_recovery_input_issue: Option<StreamIssueKind>,
    pub last_recovery_output_issue: Option<StreamIssueKind>,
    pub uptime_seconds: f64,
}

struct RouteGeneration {
    input_device: AudioDeviceDescriptor,
    output_device: AudioDeviceDescriptor,
    input_format: AudioFormat,
    output_format: AudioFormat,
    input_stream: CpalStream,
    output_stream: CpalStream,
    worker: PipelineWorkerHandle,
    metrics: RouteMetricsStore,
    input_issues: StreamIssueStore,
    output_issues: StreamIssueStore,
}

struct GenerationBuildOutcome {
    generation: RouteGeneration,
    recovery_attempts: u64,
    last_error: Option<String>,
}

struct OutputCandidate {
    device: AudioDeviceDescriptor,
    stream: CpalStream,
    producer: AudioRingProducer,
    issues: StreamIssueStore,
    enabled: Arc<AtomicBool>,
}

struct StableOutputOutcome {
    candidate: OutputCandidate,
    recovery_attempts: u64,
    last_error: Option<String>,
}

/// Owner-thread-confined physical microphone monitor route.
pub struct HeadlessMonitor {
    control: MonitorControl,
    config: HeadlessMonitorConfig,
    asset: ModelAsset,
    route_id: RouteId,
    state: RouteStateMachine,
    input_device: AudioDeviceDescriptor,
    output_device: AudioDeviceDescriptor,
    input_format: AudioFormat,
    output_format: AudioFormat,
    model: NoiseReducerDescriptor,
    input_stream: Option<CpalStream>,
    output_stream: Option<CpalStream>,
    worker: Option<PipelineWorkerHandle>,
    metrics: RouteMetricsStore,
    input_issues: StreamIssueStore,
    output_issues: StreamIssueStore,
    completed_input_issues: StreamIssueSnapshot,
    completed_output_issues: StreamIssueSnapshot,
    completed_input_callbacks: CallbackMetricsSnapshot,
    completed_output_callbacks: CallbackMetricsSnapshot,
    recovery_attempts: u64,
    successful_recoveries: u64,
    last_recovery_error: Option<String>,
    last_recovery_input_issue: Option<StreamIssueKind>,
    last_recovery_output_issue: Option<StreamIssueKind>,
    started_at: Instant,
}

impl HeadlessMonitor {
    /// Activates input first, negotiates the settled output route, and starts processing.
    pub fn start(config: HeadlessMonitorConfig) -> Result<Self, HeadlessMonitorError> {
        Self::start_controlled(config, MonitorControl::default())
    }

    /// Starts on the caller's owner thread with an out-of-band cancellation flag. Setting it to
    /// true interrupts retry/activation waits and gates PCM callbacks; call `stop` to join resources.
    pub fn start_with_cancellation(
        config: HeadlessMonitorConfig,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, HeadlessMonitorError> {
        Self::start_controlled(config, MonitorControl::with_cancellation(cancelled))
    }

    pub(crate) fn start_controlled(
        config: HeadlessMonitorConfig,
        control: MonitorControl,
    ) -> Result<Self, HeadlessMonitorError> {
        control.check()?;
        config.validate()?;
        let mut state = RouteStateMachine::new();
        state.transition(PipelineState::Starting)?;

        let asset = ModelAsset::load(&config.model_path)?;
        let model = asset.descriptor().clone();
        let outcome = build_generation_with_budget(&config, &asset, &control)?;
        state.transition(PipelineState::Running)?;
        let generation = outcome.generation;
        let startup_recovered = outcome.recovery_attempts > 0;

        control.check()?;
        Ok(Self {
            control,
            route_id: config.route_id.clone(),
            config,
            asset,
            state,
            input_device: generation.input_device,
            output_device: generation.output_device,
            input_format: generation.input_format,
            output_format: generation.output_format,
            model,
            input_stream: Some(generation.input_stream),
            output_stream: Some(generation.output_stream),
            worker: Some(generation.worker),
            metrics: generation.metrics,
            input_issues: generation.input_issues,
            output_issues: generation.output_issues,
            completed_input_issues: StreamIssueSnapshot::default(),
            completed_output_issues: StreamIssueSnapshot::default(),
            completed_input_callbacks: CallbackMetricsSnapshot::default(),
            completed_output_callbacks: CallbackMetricsSnapshot::default(),
            recovery_attempts: outcome.recovery_attempts,
            successful_recoveries: u64::from(startup_recovered),
            last_recovery_error: outcome.last_error,
            last_recovery_input_issue: None,
            last_recovery_output_issue: None,
            started_at: Instant::now(),
        })
    }

    pub fn set_bypass(&mut self, bypassed: bool) -> Result<(), HeadlessMonitorError> {
        self.running_worker()?.set_bypass(bypassed)?;
        self.config.initially_bypassed = bypassed;
        Ok(())
    }

    pub fn update_settings(
        &mut self,
        settings: DenoiseSettings,
    ) -> Result<(), HeadlessMonitorError> {
        settings
            .validate()
            .map_err(|_| HeadlessMonitorError::InvalidConfiguration("invalid denoise settings"))?;
        self.running_worker()?.update_settings(settings.clone())?;
        self.config.denoise = settings;
        Ok(())
    }

    /// Stops input production, joins the worker, then releases output.
    pub fn stop(&mut self) -> Result<(), HeadlessMonitorError> {
        if self.state.state() == PipelineState::Stopped {
            return Ok(());
        }
        self.state.transition(PipelineState::Stopping)?;
        let worker_result = self.release_resources();
        self.state.transition(PipelineState::Stopped)?;
        worker_result.map_err(HeadlessMonitorError::Worker)
    }

    #[must_use]
    pub fn snapshot(&self) -> HeadlessMonitorSnapshot {
        let active_input_callbacks = self.input_stream.as_ref().map_or_else(
            CallbackMetricsSnapshot::default,
            CpalStream::callback_metrics,
        );
        let active_output_callbacks = self.output_stream.as_ref().map_or_else(
            CallbackMetricsSnapshot::default,
            CpalStream::callback_metrics,
        );
        let input_callbacks =
            add_callback_metrics(self.completed_input_callbacks, active_input_callbacks);
        let output_callbacks =
            add_callback_metrics(self.completed_output_callbacks, active_output_callbacks);
        let input_issues =
            add_issue_snapshots(self.completed_input_issues, self.input_issues.snapshot());
        let output_issues =
            add_issue_snapshots(self.completed_output_issues, self.output_issues.snapshot());
        HeadlessMonitorSnapshot {
            route_id: self.route_id.clone(),
            state: self.state.state(),
            input_device: self.input_device.clone(),
            output_device: self.output_device.clone(),
            input_format: self.input_format.clone(),
            output_format: self.output_format.clone(),
            model: self.model.clone(),
            metrics: self.metrics.snapshot(),
            model_errors: self.metrics.model_errors(),
            input_issues,
            output_issues,
            input_callbacks,
            output_callbacks,
            recovery_attempts: self.recovery_attempts,
            successful_recoveries: self.successful_recoveries,
            last_recovery_error: self.last_recovery_error.clone(),
            last_recovery_input_issue: self.last_recovery_input_issue,
            last_recovery_output_issue: self.last_recovery_output_issue,
            uptime_seconds: self.started_at.elapsed().as_secs_f64(),
        }
    }

    fn running_worker(&self) -> Result<&PipelineWorkerHandle, HeadlessMonitorError> {
        if self.state.state() != PipelineState::Running {
            return Err(StateTransitionError {
                from: self.state.state(),
                to: PipelineState::Running,
            }
            .into());
        }
        self.worker
            .as_ref()
            .ok_or(HeadlessMonitorError::Worker(WorkerError::Panicked))
    }

    /// Advances health checks and recovery on the route owner thread; snapshots never do work.
    pub fn poll_health(&mut self) {
        if self.control.is_cancelled() {
            return;
        }
        if self.state.state() != PipelineState::Running {
            return;
        }
        let worker_finished = self
            .worker
            .as_ref()
            .is_none_or(PipelineWorkerHandle::is_finished);
        let input = self.input_issues.snapshot();
        let output = self.output_issues.snapshot();
        let fault = if let Some(kind) = input.last_terminal_kind {
            Some((StreamDirection::Input, kind, stream_issue_action(kind)))
        } else {
            output
                .last_terminal_kind
                .map(|kind| (StreamDirection::Output, kind, stream_issue_action(kind)))
        };

        if let Some((direction, kind, RecoveryAction::Fail)) = fault {
            self.last_recovery_error =
                Some(HeadlessMonitorError::StreamFault { direction, kind }.to_string());
            let _ = self.state.transition(PipelineState::Failed);
            let _ = self.release_resources();
            return;
        }

        if worker_finished || fault.is_some() {
            self.last_recovery_input_issue = input.last_terminal_kind;
            self.last_recovery_output_issue = output.last_terminal_kind;
            let action = fault.map_or(RecoveryAction::Retry, |(_, _, action)| action);
            self.recover_route(action);
        }
    }

    fn recover_route(&mut self, action: RecoveryAction) {
        let recovery_state = if action == RecoveryAction::Reconfigure {
            PipelineState::Reconfiguring
        } else {
            PipelineState::Recovering
        };
        if self.state.transition(recovery_state).is_err() {
            return;
        }

        self.control.publish(recovery_state);
        if let Some(kind) = self.last_recovery_input_issue {
            self.last_recovery_error = Some(
                HeadlessMonitorError::StreamFault {
                    direction: StreamDirection::Input,
                    kind,
                }
                .to_string(),
            );
        } else if let Some(kind) = self.last_recovery_output_issue {
            self.last_recovery_error = Some(
                HeadlessMonitorError::StreamFault {
                    direction: StreamDirection::Output,
                    kind,
                }
                .to_string(),
            );
        } else {
            self.last_recovery_error = Some("DSP worker stopped unexpectedly".to_owned());
        }

        if let Err(error) = self.release_resources() {
            self.last_recovery_error = Some(error.to_string());
        }

        let policy = self.config.recovery;
        for attempt in RecoveryBudget::new(policy) {
            self.recovery_attempts = self.recovery_attempts.saturating_add(1);
            if self.control.wait(attempt.delay).is_err() {
                return;
            }
            match build_generation(&self.config, &self.asset, &self.control) {
                Ok(outcome) => {
                    self.recovery_attempts = self
                        .recovery_attempts
                        .saturating_add(outcome.recovery_attempts);
                    if outcome.last_error.is_some() {
                        self.last_recovery_error = outcome.last_error;
                    }
                    self.install_generation(outcome.generation);
                    self.successful_recoveries = self.successful_recoveries.saturating_add(1);
                    let _ = self.state.transition(PipelineState::Running);
                    return;
                }
                Err(HeadlessMonitorError::Cancelled) => return,
                Err(error) => {
                    let next_action = headless_error_action(&error);
                    self.last_recovery_error = Some(error.to_string());
                    if next_action == RecoveryAction::Fail {
                        break;
                    }
                    if self.state.state() == PipelineState::Reconfiguring {
                        let _ = self.state.transition(PipelineState::Recovering);
                        self.control.publish(PipelineState::Recovering);
                    }
                }
            }
        }
        let _ = self.state.transition(PipelineState::Failed);
    }

    fn install_generation(&mut self, generation: RouteGeneration) {
        self.input_device = generation.input_device;
        self.output_device = generation.output_device;
        self.input_format = generation.input_format;
        self.output_format = generation.output_format;
        self.input_stream = Some(generation.input_stream);
        self.output_stream = Some(generation.output_stream);
        self.worker = Some(generation.worker);
        self.metrics = generation.metrics;
        self.input_issues = generation.input_issues;
        self.output_issues = generation.output_issues;
    }

    fn release_resources(&mut self) -> Result<(), WorkerError> {
        if let Some(stream) = self.input_stream.take() {
            self.completed_input_callbacks =
                add_callback_metrics(self.completed_input_callbacks, stream.callback_metrics());
            drop(stream);
        }
        let worker_result = match self.worker.as_mut() {
            Some(worker) => worker.shutdown(),
            None => Ok(()),
        };
        self.worker = None;
        if let Some(stream) = self.output_stream.take() {
            self.completed_output_callbacks =
                add_callback_metrics(self.completed_output_callbacks, stream.callback_metrics());
            drop(stream);
        }
        let input_issues = std::mem::take(&mut self.input_issues).snapshot();
        let output_issues = std::mem::take(&mut self.output_issues).snapshot();
        self.completed_input_issues =
            add_issue_snapshots(self.completed_input_issues, input_issues);
        self.completed_output_issues =
            add_issue_snapshots(self.completed_output_issues, output_issues);
        worker_result
    }
}

fn build_generation_with_budget(
    config: &HeadlessMonitorConfig,
    asset: &ModelAsset,
    control: &MonitorControl,
) -> Result<GenerationBuildOutcome, HeadlessMonitorError> {
    let mut last_error = None;
    for attempt in RecoveryBudget::new(config.recovery) {
        if !attempt.delay.is_zero() {
            control.wait(attempt.delay)?;
        }
        match build_generation(config, asset, control) {
            Ok(mut outcome) => {
                outcome.recovery_attempts = outcome
                    .recovery_attempts
                    .saturating_add(u64::from(attempt.number.saturating_sub(1)));
                if outcome.last_error.is_none() {
                    outcome.last_error = last_error;
                }
                return Ok(outcome);
            }
            Err(error) => {
                if headless_error_action(&error) == RecoveryAction::Fail {
                    return Err(error);
                }
                last_error = Some(error.to_string());
                if attempt.number == config.recovery.max_attempts {
                    return Err(HeadlessMonitorError::RecoveryExhausted {
                        attempts: attempt.number,
                        last_error: last_error.unwrap_or_else(|| "unknown route error".to_owned()),
                    });
                }
            }
        }
    }
    unreachable!("a validated recovery budget has at least one attempt")
}

fn build_generation(
    config: &HeadlessMonitorConfig,
    asset: &ModelAsset,
    control: &MonitorControl,
) -> Result<GenerationBuildOutcome, HeadlessMonitorError> {
    control.check()?;
    let host = CpalHost::new_default();
    let input_preferences = NegotiationPreferences::new(
        asset.descriptor().sample_rate_hz,
        ChannelCount::new(1).map_err(|_| {
            HeadlessMonitorError::InvalidConfiguration("invalid mono channel count")
        })?,
    );
    let output_preferences = NegotiationPreferences::new(
        asset.descriptor().sample_rate_hz,
        ChannelCount::new(2).map_err(|_| {
            HeadlessMonitorError::InvalidConfiguration("invalid stereo channel count")
        })?,
    );

    // Some devices change their hardware profile when capture is activated. Build and start the
    // input first, but gate PCM until the rest of this route generation is ready. Output
    // negotiation therefore observes the post-activation device configuration.
    let input_device = host.resolve_input(&config.input)?;
    let input_preferences = prefer_native_format(
        input_preferences,
        input_device.descriptor().default_input_format.as_ref(),
    );
    let input_config = input_device.negotiate_input(input_preferences)?;
    let input_capacity_frames = frames_for_ms(
        input_config.protocol().sample_rate_hz,
        config.input_capacity_ms,
    )?;
    let (mut input_producer, input_consumer) =
        bounded_audio_ring(input_capacity_frames, input_config.protocol().channels)?;
    let capture_enabled = Arc::new(AtomicBool::new(false));
    let worker_wake = Arc::new(OnceLock::<PipelineWorkerWake>::new());
    let input_issues = StreamIssueStore::default();
    let input_callback_issues = input_issues.clone();
    let callback_capture_enabled = capture_enabled.clone();
    let callback_cancelled = control.cancelled.clone();
    let callback_worker_wake = worker_wake.clone();
    control.check()?;
    let input_stream = input_device.build_input_stream(
        &input_config,
        move |samples| {
            if callback_cancelled.load(Ordering::Acquire)
                || !callback_capture_enabled.load(Ordering::Acquire)
            {
                return;
            }
            if input_producer.write(samples).is_err() {
                input_callback_issues.record_callback_violation();
            }
            if let Some(wake) = callback_worker_wake.get() {
                wake.wake();
            }
        },
        input_issues.clone(),
    )?;
    control.check()?;
    input_stream.play()?;
    wait_for_stream_activation(
        &input_stream,
        &input_issues,
        StreamDirection::Input,
        config.recovery,
        control,
    )?;

    // Keep capture active while the output is retried. Bluetooth duplex endpoints can disappear
    // briefly while the input stream moves the headset from high-quality playback to its duplex
    // profile; tearing the input down between retries can make that transition oscillate forever.
    let stable_output = build_stable_output(
        config,
        output_preferences,
        &input_stream,
        &input_issues,
        control,
    )?;
    let OutputCandidate {
        device: output_device,
        stream: output_stream,
        producer: output_producer,
        issues: output_issues,
        enabled: output_enabled,
    } = stable_output.candidate;
    let output_format = output_stream.format().clone();
    let input_stale_target_frames = frames_for_ms(
        input_config.protocol().sample_rate_hz,
        config.input_stale_target_ms,
    )?;

    let metrics = RouteMetricsStore::new(
        input_consumer.metrics(),
        output_producer.metrics(),
        input_config.protocol().clone(),
        output_format.clone(),
    );
    let mut worker_config = PipelineWorkerConfig::new(
        input_config.protocol().clone(),
        output_format.clone(),
        input_stale_target_frames,
    );
    worker_config.initially_bypassed = config.initially_bypassed;
    worker_config.denoise_enabled = config.denoise.enabled;
    worker_config.crossfade_ms = u32::from(config.crossfade_ms);
    worker_config.output_target_fill_frames =
        frames_for_ms(output_format.sample_rate_hz, config.target_latency_ms)?;
    let reducer_asset = asset.clone();
    let reducer_settings = config.denoise.clone();
    let worker = spawn_pipeline_worker_controlled(
        worker_config,
        input_consumer,
        output_producer,
        metrics.clone(),
        move || {
            let mut reducer = DfnNoiseReducer::from_asset(reducer_asset, reducer_settings)?;
            reducer.warm_up(MODEL_WARM_UP_FRAMES)?;
            Ok(reducer)
        },
        &control.cancelled,
    )
    .map_err(|error| match error {
        WorkerStartError::Cancelled => HeadlessMonitorError::Cancelled,
        error => HeadlessMonitorError::WorkerStart(error),
    })?;
    let _ = worker_wake.set(worker.waker());

    control.check()?;
    capture_enabled.store(true, Ordering::Release);
    output_enabled.store(true, Ordering::Release);
    worker.wake();
    wait_for_route_activation(
        &input_stream,
        &input_issues,
        &output_stream,
        &output_issues,
        &worker,
        config.recovery,
        control,
    )?;

    Ok(GenerationBuildOutcome {
        generation: RouteGeneration {
            input_device: input_device.descriptor().clone(),
            output_device,
            input_format: input_stream.format().clone(),
            output_format,
            input_stream,
            output_stream,
            worker,
            metrics,
            input_issues,
            output_issues,
        },
        recovery_attempts: stable_output.recovery_attempts,
        last_error: stable_output.last_error,
    })
}

fn build_stable_output(
    config: &HeadlessMonitorConfig,
    preferences: NegotiationPreferences,
    input_stream: &CpalStream,
    input_issues: &StreamIssueStore,
    control: &MonitorControl,
) -> Result<StableOutputOutcome, HeadlessMonitorError> {
    let mut last_error = None;
    for attempt in RecoveryBudget::new(config.recovery) {
        if !attempt.delay.is_zero() {
            control.wait(attempt.delay)?;
        }
        match build_output_candidate(config, preferences, input_stream, input_issues, control) {
            Ok(candidate) => {
                return Ok(StableOutputOutcome {
                    candidate,
                    recovery_attempts: u64::from(attempt.number.saturating_sub(1)),
                    last_error,
                });
            }
            Err(error) => {
                let input_failed = matches!(
                    error,
                    HeadlessMonitorError::ActivationTimeout {
                        direction: StreamDirection::Input
                    } | HeadlessMonitorError::StreamFault {
                        direction: StreamDirection::Input,
                        ..
                    }
                );
                if input_failed || headless_error_action(&error) == RecoveryAction::Fail {
                    return Err(error);
                }
                last_error = Some(error.to_string());
                if attempt.number == config.recovery.max_attempts {
                    return Err(HeadlessMonitorError::RecoveryExhausted {
                        attempts: attempt.number,
                        last_error: last_error
                            .unwrap_or_else(|| "unknown output route error".to_owned()),
                    });
                }
            }
        }
    }
    unreachable!("a validated recovery budget has at least one attempt")
}

fn build_output_candidate(
    config: &HeadlessMonitorConfig,
    preferences: NegotiationPreferences,
    input_stream: &CpalStream,
    input_issues: &StreamIssueStore,
    control: &MonitorControl,
) -> Result<OutputCandidate, HeadlessMonitorError> {
    control.check()?;
    // Re-enumerate on every attempt so a profile switch or default-route change cannot leave this
    // stage holding a stale backend device handle or stale supported-format list.
    let host = CpalHost::new_default();
    let device = host.resolve_output(&config.output)?;
    let preferences = prefer_native_format(
        preferences,
        device.descriptor().default_output_format.as_ref(),
    );
    let output_config = device.negotiate_output(preferences)?;
    let output_capacity_frames = frames_for_ms(
        output_config.protocol().sample_rate_hz,
        config.output_capacity_ms,
    )?;
    let output_prefill_frames = frames_for_ms(
        output_config.protocol().sample_rate_hz,
        config.target_latency_ms,
    )?;
    let (mut producer, mut consumer) =
        bounded_audio_ring(output_capacity_frames, output_config.protocol().channels)?;
    let prefill_samples = output_prefill_frames
        .checked_mul(output_config.protocol().channels.as_usize())
        .ok_or(HeadlessMonitorError::QueueSizeOverflow)?;
    producer.write(&vec![0.0; prefill_samples])?;

    let enabled = Arc::new(AtomicBool::new(false));
    control.check()?;
    let callback_enabled = enabled.clone();
    let callback_cancelled = control.cancelled.clone();
    let issues = StreamIssueStore::default();
    let callback_issues = issues.clone();
    let stream = device.build_output_stream(
        &output_config,
        move |samples| {
            if callback_cancelled.load(Ordering::Acquire)
                || !callback_enabled.load(Ordering::Acquire)
            {
                samples.fill(0.0);
                return;
            }
            if consumer.read_or_silence(samples).is_err() {
                callback_issues.record_callback_violation();
            }
        },
        issues.clone(),
    )?;
    control.check()?;
    stream.play()?;
    wait_for_output_activation(
        input_stream,
        input_issues,
        &stream,
        &issues,
        config.recovery,
        control,
    )?;

    Ok(OutputCandidate {
        device: device.descriptor().clone(),
        stream,
        producer,
        issues,
        enabled,
    })
}

fn wait_for_stream_activation(
    stream: &CpalStream,
    issues: &StreamIssueStore,
    direction: StreamDirection,
    policy: RouteRecoveryPolicy,
    control: &MonitorControl,
) -> Result<(), HeadlessMonitorError> {
    let deadline = Instant::now() + policy.activation_timeout;
    let baseline_callbacks = stream.callback_metrics().callbacks;
    let mut activation_observation = None;
    loop {
        control.check()?;
        fail_on_terminal_issue(issues, direction)?;
        let now = Instant::now();
        let callbacks = stream.callback_metrics().callbacks;
        if callbacks > baseline_callbacks {
            let (observed_at, observed_callbacks) =
                activation_observation.get_or_insert((now, callbacks));
            if now.duration_since(*observed_at) >= policy.settle_period
                && callbacks > *observed_callbacks
            {
                return Ok(());
            }
        }
        if now >= deadline {
            return Err(HeadlessMonitorError::ActivationTimeout { direction });
        }
        control.wait(
            policy
                .poll_interval
                .min(deadline.saturating_duration_since(now)),
        )?;
    }
}

fn wait_for_output_activation(
    input_stream: &CpalStream,
    input_issues: &StreamIssueStore,
    output_stream: &CpalStream,
    output_issues: &StreamIssueStore,
    policy: RouteRecoveryPolicy,
    control: &MonitorControl,
) -> Result<(), HeadlessMonitorError> {
    let deadline = Instant::now() + policy.activation_timeout;
    let baseline_callbacks = output_stream.callback_metrics().callbacks;
    let mut activation_observation = None;
    loop {
        control.check()?;
        fail_on_terminal_issue(input_issues, StreamDirection::Input)?;
        fail_on_terminal_issue(output_issues, StreamDirection::Output)?;
        let now = Instant::now();
        let output_callbacks = output_stream.callback_metrics().callbacks;
        if input_stream.callback_metrics().callbacks > 0 && output_callbacks > baseline_callbacks {
            let (observed_at, observed_callbacks) =
                activation_observation.get_or_insert((now, output_callbacks));
            if now.duration_since(*observed_at) >= policy.settle_period
                && output_callbacks > *observed_callbacks
            {
                return Ok(());
            }
        }
        if now >= deadline {
            return Err(HeadlessMonitorError::ActivationTimeout {
                direction: StreamDirection::Output,
            });
        }
        control.wait(
            policy
                .poll_interval
                .min(deadline.saturating_duration_since(now)),
        )?;
    }
}

fn wait_for_route_activation(
    input_stream: &CpalStream,
    input_issues: &StreamIssueStore,
    output_stream: &CpalStream,
    output_issues: &StreamIssueStore,
    worker: &PipelineWorkerHandle,
    policy: RouteRecoveryPolicy,
    control: &MonitorControl,
) -> Result<(), HeadlessMonitorError> {
    let deadline = Instant::now() + policy.activation_timeout;
    let baseline_input_callbacks = input_stream.callback_metrics().callbacks;
    let baseline_output_callbacks = output_stream.callback_metrics().callbacks;
    let mut activation_observation = None;
    loop {
        control.check()?;
        fail_on_terminal_issue(input_issues, StreamDirection::Input)?;
        fail_on_terminal_issue(output_issues, StreamDirection::Output)?;
        if worker.is_finished() {
            return Err(HeadlessMonitorError::WorkerStoppedDuringActivation);
        }
        let now = Instant::now();
        let input_callbacks = input_stream.callback_metrics().callbacks;
        let output_callbacks = output_stream.callback_metrics().callbacks;
        if input_callbacks > baseline_input_callbacks
            && output_callbacks > baseline_output_callbacks
        {
            let (observed_at, observed_input, observed_output) =
                activation_observation.get_or_insert((now, input_callbacks, output_callbacks));
            if now.duration_since(*observed_at) >= policy.settle_period
                && input_callbacks > *observed_input
                && output_callbacks > *observed_output
            {
                return Ok(());
            }
        }
        if now >= deadline {
            return Err(HeadlessMonitorError::ActivationTimeout {
                direction: StreamDirection::Output,
            });
        }
        control.wait(
            policy
                .poll_interval
                .min(deadline.saturating_duration_since(now)),
        )?;
    }
}

fn fail_on_terminal_issue(
    issues: &StreamIssueStore,
    direction: StreamDirection,
) -> Result<(), HeadlessMonitorError> {
    let snapshot = issues.snapshot();
    match snapshot.last_terminal_kind {
        Some(kind) => Err(HeadlessMonitorError::StreamFault { direction, kind }),
        None => Ok(()),
    }
}

fn headless_error_action(error: &HeadlessMonitorError) -> RecoveryAction {
    match error {
        HeadlessMonitorError::Cancelled => RecoveryAction::Fail,
        HeadlessMonitorError::AudioIo(error) => audio_error_action(error),
        HeadlessMonitorError::ActivationTimeout { .. }
        | HeadlessMonitorError::WorkerStoppedDuringActivation => RecoveryAction::Retry,
        HeadlessMonitorError::StreamFault { kind, .. } => stream_issue_action(*kind),
        HeadlessMonitorError::Denoise(_)
        | HeadlessMonitorError::Transport(_)
        | HeadlessMonitorError::WorkerStart(_)
        | HeadlessMonitorError::Worker(_)
        | HeadlessMonitorError::WorkerCommand(_)
        | HeadlessMonitorError::State(_)
        | HeadlessMonitorError::InvalidConfiguration(_)
        | HeadlessMonitorError::QueueSizeOverflow
        | HeadlessMonitorError::RecoveryExhausted { .. } => RecoveryAction::Fail,
    }
}

fn add_callback_metrics(
    left: CallbackMetricsSnapshot,
    right: CallbackMetricsSnapshot,
) -> CallbackMetricsSnapshot {
    CallbackMetricsSnapshot {
        callbacks: left.callbacks.saturating_add(right.callbacks),
        frames: left.frames.saturating_add(right.frames),
    }
}

fn add_issue_snapshots(
    left: StreamIssueSnapshot,
    right: StreamIssueSnapshot,
) -> StreamIssueSnapshot {
    StreamIssueSnapshot {
        total: left.total.saturating_add(right.total),
        xruns: left.xruns.saturating_add(right.xruns),
        terminal: left.terminal.saturating_add(right.terminal),
        callback_violations: left
            .callback_violations
            .saturating_add(right.callback_violations),
        last_kind: right.last_kind.or(left.last_kind),
        last_terminal_kind: right.last_terminal_kind.or(left.last_terminal_kind),
    }
}

fn prefer_native_format(
    preferences: NegotiationPreferences,
    native: Option<&AudioFormat>,
) -> NegotiationPreferences {
    native.map_or(preferences, |native| NegotiationPreferences {
        sample_rate_hz: native.sample_rate_hz,
        channels: native.channels,
        buffer_size_frames: preferences.buffer_size_frames,
    })
}

impl Drop for HeadlessMonitor {
    fn drop(&mut self) {
        let _ = self.release_resources();
    }
}

fn frames_for_ms(rate: SampleRateHz, milliseconds: u16) -> Result<usize, HeadlessMonitorError> {
    let frames = u128::from(rate.get())
        .saturating_mul(u128::from(milliseconds))
        .div_ceil(1_000);
    usize::try_from(frames).map_err(|_| HeadlessMonitorError::QueueSizeOverflow)
}

#[cfg(test)]
mod tests {
    use audio_protocol::SampleFormat;

    use super::*;

    #[test]
    fn defaults_have_bounded_consistent_queue_durations() {
        let config = HeadlessMonitorConfig::default();
        config.validate().expect("default config is valid");
        assert!(config.target_latency_ms <= config.output_capacity_ms);
        assert!(config.input_stale_target_ms <= config.input_capacity_ms);
        assert_eq!(
            frames_for_ms(SampleRateHz::new(44_100).expect("valid rate"), 10)
                .expect("frame conversion"),
            441
        );
    }

    #[test]
    fn invalid_queue_relationships_are_rejected() {
        let mut config = HeadlessMonitorConfig {
            target_latency_ms: 201,
            ..HeadlessMonitorConfig::default()
        };
        assert!(matches!(
            config.validate(),
            Err(HeadlessMonitorError::InvalidConfiguration(_))
        ));
        config.target_latency_ms = 40;
        config.input_stale_target_ms = 101;
        assert!(config.validate().is_err());

        config.input_stale_target_ms = 80;
        config.recovery.max_attempts = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn completed_generation_diagnostics_are_accumulated() {
        let callbacks = add_callback_metrics(
            CallbackMetricsSnapshot {
                callbacks: u64::MAX,
                frames: 10,
            },
            CallbackMetricsSnapshot {
                callbacks: 1,
                frames: 20,
            },
        );
        assert_eq!(callbacks.callbacks, u64::MAX);
        assert_eq!(callbacks.frames, 30);

        let previous = StreamIssueSnapshot {
            total: 2,
            terminal: 1,
            last_kind: Some(StreamIssueKind::Xrun),
            last_terminal_kind: Some(StreamIssueKind::StreamInvalidated),
            ..StreamIssueSnapshot::default()
        };
        let active = StreamIssueSnapshot {
            total: 1,
            xruns: 1,
            last_kind: Some(StreamIssueKind::Xrun),
            ..StreamIssueSnapshot::default()
        };
        let combined = add_issue_snapshots(previous, active);
        assert_eq!(combined.total, 3);
        assert_eq!(combined.terminal, 1);
        assert_eq!(combined.xruns, 1);
        assert_eq!(combined.last_kind, Some(StreamIssueKind::Xrun));
        assert_eq!(
            combined.last_terminal_kind,
            Some(StreamIssueKind::StreamInvalidated)
        );
    }

    #[test]
    fn native_endpoint_format_wins_without_discarding_buffer_preference() {
        let preferred = NegotiationPreferences::new(
            SampleRateHz::new(48_000).expect("valid rate"),
            ChannelCount::new(1).expect("valid channel count"),
        )
        .with_buffer_size(audio_protocol::BufferSizeFrames::new(256).expect("valid buffer size"));
        let native = AudioFormat {
            sample_rate_hz: SampleRateHz::new(24_000).expect("valid rate"),
            channels: ChannelCount::new(2).expect("valid channel count"),
            sample_format: SampleFormat::F32,
            buffer_size_frames: None,
        };

        let selected = prefer_native_format(preferred, Some(&native));
        assert_eq!(selected.sample_rate_hz, native.sample_rate_hz);
        assert_eq!(selected.channels, native.channels);
        assert_eq!(selected.buffer_size_frames, preferred.buffer_size_frames);
        assert_eq!(prefer_native_format(preferred, None), preferred);
    }
}
