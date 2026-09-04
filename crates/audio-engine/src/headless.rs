use std::{path::PathBuf, time::Instant};

use audio_cpal::{
    AudioIoError, CallbackMetricsSnapshot, CpalHost, CpalStream, DeviceSelection,
    NegotiationPreferences, StreamIssueSnapshot, StreamIssueStore,
};
use audio_protocol::{
    AudioDeviceDescriptor, AudioFormat, ChannelCount, DenoiseSettings, PipelineMetrics,
    PipelineState, RouteId, SampleRateHz,
};
use denoise_core::{DenoiseError, NoiseReducerDescriptor};
use denoise_dfn::{DfnNoiseReducer, ModelAsset, default_model_path};

use crate::{
    AudioTransportError, PipelineWorkerConfig, PipelineWorkerHandle, RouteMetricsStore,
    RouteStateMachine, StateTransitionError, WorkerCommandError, WorkerError, WorkerStartError,
    bounded_audio_ring, spawn_pipeline_worker,
};

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
            .map_err(|_| HeadlessMonitorError::InvalidConfiguration("invalid denoise settings"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HeadlessMonitorError {
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
    pub uptime_seconds: f64,
}

/// Supervisor-owned physical microphone monitor route.
pub struct HeadlessMonitor {
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
    last_input_callbacks: CallbackMetricsSnapshot,
    last_output_callbacks: CallbackMetricsSnapshot,
    started_at: Instant,
}

impl HeadlessMonitor {
    /// Resolves devices, starts the worker, then starts output before input.
    pub fn start(config: HeadlessMonitorConfig) -> Result<Self, HeadlessMonitorError> {
        config.validate()?;
        let mut state = RouteStateMachine::new();
        state.transition(PipelineState::Starting)?;

        let asset = ModelAsset::load(&config.model_path)?;
        let model = asset.descriptor().clone();
        let host = CpalHost::new_default();
        let input_device = host.resolve_input(&config.input)?;
        let output_device = host.resolve_output(&config.output)?;
        let input_preferences = NegotiationPreferences::new(
            model.sample_rate_hz,
            ChannelCount::new(1).map_err(|_| {
                HeadlessMonitorError::InvalidConfiguration("invalid mono channel count")
            })?,
        );
        let output_preferences = NegotiationPreferences::new(
            model.sample_rate_hz,
            ChannelCount::new(2).map_err(|_| {
                HeadlessMonitorError::InvalidConfiguration("invalid stereo channel count")
            })?,
        );
        let input_config = input_device.negotiate_input(input_preferences)?;
        let output_config = output_device.negotiate_output(output_preferences)?;
        let input_capacity_frames = frames_for_ms(
            input_config.protocol().sample_rate_hz,
            config.input_capacity_ms,
        )?;
        let output_capacity_frames = frames_for_ms(
            output_config.protocol().sample_rate_hz,
            config.output_capacity_ms,
        )?;
        let input_stale_target_frames = frames_for_ms(
            input_config.protocol().sample_rate_hz,
            config.input_stale_target_ms,
        )?;
        let output_prefill_frames = frames_for_ms(
            output_config.protocol().sample_rate_hz,
            config.target_latency_ms,
        )?;

        let (mut input_producer, input_consumer) =
            bounded_audio_ring(input_capacity_frames, input_config.protocol().channels)?;
        let (mut output_producer, mut output_consumer) =
            bounded_audio_ring(output_capacity_frames, output_config.protocol().channels)?;
        let prefill_samples = output_prefill_frames
            .checked_mul(output_config.protocol().channels.as_usize())
            .ok_or(HeadlessMonitorError::QueueSizeOverflow)?;
        let prefill = vec![0.0; prefill_samples];
        output_producer.write(&prefill)?;

        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            input_config.protocol().clone(),
            output_config.protocol().clone(),
        );
        let mut worker_config = PipelineWorkerConfig::new(
            input_config.protocol().clone(),
            output_config.protocol().clone(),
            input_stale_target_frames,
        );
        worker_config.initially_bypassed = config.initially_bypassed;
        worker_config.denoise_enabled = config.denoise.enabled;
        worker_config.crossfade_ms = u32::from(config.crossfade_ms);
        let reducer_settings = config.denoise.clone();
        let mut worker = spawn_pipeline_worker(
            worker_config,
            input_consumer,
            output_producer,
            metrics.clone(),
            move || {
                let mut reducer = DfnNoiseReducer::from_asset(asset, reducer_settings)?;
                reducer.warm_up(MODEL_WARM_UP_FRAMES)?;
                Ok(reducer)
            },
        )?;

        let input_issues = StreamIssueStore::default();
        let output_issues = StreamIssueStore::default();
        let output_callback_issues = output_issues.clone();
        let output_stream = output_device.build_output_stream(
            &output_config,
            move |samples| {
                if output_consumer.read_or_silence(samples).is_err() {
                    output_callback_issues.record_callback_violation();
                }
            },
            output_issues.clone(),
        )?;

        let wake = worker.waker();
        let input_callback_issues = input_issues.clone();
        let input_stream = input_device.build_input_stream(
            &input_config,
            move |samples| {
                if input_producer.write(samples).is_err() {
                    input_callback_issues.record_callback_violation();
                }
                wake.wake();
            },
            input_issues.clone(),
        )?;

        // Both streams are fully constructed before either one starts. This keeps device setup
        // from consuming the bounded output prefill, while preserving output-before-input order.
        output_stream.play()?;
        if let Err(error) = input_stream.play() {
            let _ = worker.shutdown();
            return Err(error.into());
        }
        state.transition(PipelineState::Running)?;

        Ok(Self {
            route_id: config.route_id,
            state,
            input_device: input_device.descriptor().clone(),
            output_device: output_device.descriptor().clone(),
            input_format: input_stream.format().clone(),
            output_format: output_stream.format().clone(),
            model,
            input_stream: Some(input_stream),
            output_stream: Some(output_stream),
            worker: Some(worker),
            metrics,
            input_issues,
            output_issues,
            last_input_callbacks: CallbackMetricsSnapshot::default(),
            last_output_callbacks: CallbackMetricsSnapshot::default(),
            started_at: Instant::now(),
        })
    }

    pub fn set_bypass(&mut self, bypassed: bool) -> Result<(), HeadlessMonitorError> {
        self.poll_health();
        self.running_worker()?.set_bypass(bypassed)?;
        Ok(())
    }

    pub fn update_settings(
        &mut self,
        settings: DenoiseSettings,
    ) -> Result<(), HeadlessMonitorError> {
        self.poll_health();
        settings
            .validate()
            .map_err(|_| HeadlessMonitorError::InvalidConfiguration("invalid denoise settings"))?;
        self.running_worker()?.update_settings(settings)?;
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
    pub fn snapshot(&mut self) -> HeadlessMonitorSnapshot {
        self.poll_health();
        let input_callbacks = self
            .input_stream
            .as_ref()
            .map_or(self.last_input_callbacks, CpalStream::callback_metrics);
        let output_callbacks = self
            .output_stream
            .as_ref()
            .map_or(self.last_output_callbacks, CpalStream::callback_metrics);
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
            input_issues: self.input_issues.snapshot(),
            output_issues: self.output_issues.snapshot(),
            input_callbacks,
            output_callbacks,
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

    fn poll_health(&mut self) {
        if self.state.state() != PipelineState::Running {
            return;
        }
        let worker_finished = self
            .worker
            .as_ref()
            .is_none_or(PipelineWorkerHandle::is_finished);
        let terminal_stream_issue =
            self.input_issues.snapshot().terminal > 0 || self.output_issues.snapshot().terminal > 0;
        if worker_finished || terminal_stream_issue {
            let _ = self.state.transition(PipelineState::Failed);
            let _ = self.release_resources();
        }
    }

    fn release_resources(&mut self) -> Result<(), WorkerError> {
        if let Some(stream) = self.input_stream.take() {
            self.last_input_callbacks = stream.callback_metrics();
            drop(stream);
        }
        let worker_result = match self.worker.as_mut() {
            Some(worker) => worker.shutdown(),
            None => Ok(()),
        };
        self.worker = None;
        if let Some(stream) = self.output_stream.take() {
            self.last_output_callbacks = stream.callback_metrics();
            drop(stream);
        }
        worker_result
    }
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
    }
}
