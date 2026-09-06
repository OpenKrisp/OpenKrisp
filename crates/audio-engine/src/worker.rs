use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

use audio_dsp::{
    ChannelLayout, ChannelMapper, DspError, EqualPowerCrossfade, FixedDryDelay, LevelMeter,
    ModelFrameAssembler, SafetyLimiter, StreamingResampler,
};
use audio_protocol::{AudioFormat, DenoiseSettings};
use denoise_core::{DenoiseError, NoiseReducer, NoiseReducerDescriptor};

use crate::{
    AudioRingConsumer, AudioRingProducer, AudioTransportError, RouteMetricsStore,
    drift::{DriftController, DriftControllerConfig},
    overload::{OverloadGuard, OverloadTransition},
};

const DEFAULT_CONTROL_CAPACITY: usize = 8;
const DEFAULT_CROSSFADE_MS: u32 = 20;
const IDLE_PARK: Duration = Duration::from_millis(2);
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const INFERENCE_WINDOW: usize = 128;

/// Fixed graph configuration known before the reducer is constructed on its worker.
#[derive(Clone, Debug)]
pub struct PipelineWorkerConfig {
    pub input_format: AudioFormat,
    pub output_format: AudioFormat,
    pub input_stale_target_frames: usize,
    pub output_target_fill_frames: usize,
    pub initially_bypassed: bool,
    pub denoise_enabled: bool,
    pub crossfade_ms: u32,
}

impl PipelineWorkerConfig {
    #[must_use]
    pub fn new(
        input_format: AudioFormat,
        output_format: AudioFormat,
        input_stale_target_frames: usize,
    ) -> Self {
        let output_target_fill_frames =
            (u64::from(output_format.sample_rate_hz.get()) * 40 / 1_000).max(1) as usize;
        Self {
            input_format,
            output_format,
            input_stale_target_frames,
            output_target_fill_frames,
            initially_bypassed: false,
            denoise_enabled: true,
            crossfade_ms: DEFAULT_CROSSFADE_MS,
        }
    }
}

#[derive(Clone, Debug)]
pub enum WorkerCommand {
    UpdateSettings(DenoiseSettings),
    SetBypass(bool),
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorkerCommandError {
    #[error("worker control queue is full")]
    QueueFull,
    #[error("worker has stopped")]
    Stopped,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum WorkerError {
    #[error(transparent)]
    Dsp(#[from] DspError),
    #[error(transparent)]
    Transport(#[from] AudioTransportError),
    #[error(transparent)]
    Denoise(#[from] DenoiseError),
    #[error("the headless worker currently requires a mono reducer")]
    UnsupportedModelChannels,
    #[error("the output fill target must be greater than zero frames")]
    ZeroOutputFillTarget,
    #[error("output fill target {target_frames} exceeds ring capacity {capacity_frames}")]
    OutputFillTargetExceedsCapacity {
        target_frames: usize,
        capacity_frames: usize,
    },
    #[error("resampler made no progress while input remained")]
    ResamplerStalled,
    #[error("worker thread panicked")]
    Panicked,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerStartError {
    #[error("could not spawn DSP worker: {0}")]
    ThreadSpawn(String),
    #[error("DSP worker readiness timed out")]
    ReadinessTimeout,
    #[error("DSP worker exited before reporting readiness")]
    ReadinessDisconnected,
    #[error("DSP worker initialization failed: {0}")]
    Initialization(WorkerError),
}

/// Control and ownership handle for one joined DSP worker thread.
pub struct PipelineWorkerHandle {
    commands: SyncSender<WorkerCommand>,
    cancelled: Arc<AtomicBool>,
    thread: Thread,
    join: Option<JoinHandle<Result<(), WorkerError>>>,
    descriptor: NoiseReducerDescriptor,
}

/// Cloneable, allocation-free notification captured by the input callback.
#[derive(Clone, Debug)]
pub struct PipelineWorkerWake {
    thread: Thread,
}

impl PipelineWorkerWake {
    pub fn wake(&self) {
        self.thread.unpark();
    }
}

impl PipelineWorkerHandle {
    pub fn try_send(&self, command: WorkerCommand) -> Result<(), WorkerCommandError> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => WorkerCommandError::QueueFull,
                TrySendError::Disconnected(_) => WorkerCommandError::Stopped,
            })?;
        self.thread.unpark();
        Ok(())
    }

    pub fn set_bypass(&self, bypassed: bool) -> Result<(), WorkerCommandError> {
        self.try_send(WorkerCommand::SetBypass(bypassed))
    }

    pub fn update_settings(&self, settings: DenoiseSettings) -> Result<(), WorkerCommandError> {
        self.try_send(WorkerCommand::UpdateSettings(settings))
    }

    pub fn wake(&self) {
        self.thread.unpark();
    }

    #[must_use]
    pub fn waker(&self) -> PipelineWorkerWake {
        PipelineWorkerWake {
            thread: self.thread.clone(),
        }
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_none_or(JoinHandle::is_finished)
    }

    #[must_use]
    pub const fn descriptor(&self) -> &NoiseReducerDescriptor {
        &self.descriptor
    }

    pub fn shutdown(&mut self) -> Result<(), WorkerError> {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.commands.try_send(WorkerCommand::Shutdown);
        self.thread.unpark();
        join_worker(&mut self.join)
    }
}

impl Drop for PipelineWorkerHandle {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.thread.unpark();
        let _ = join_worker(&mut self.join);
    }
}

/// Spawns a worker and invokes `reducer_factory` only inside the new thread.
pub fn spawn_pipeline_worker<R, F>(
    config: PipelineWorkerConfig,
    input: AudioRingConsumer,
    output: AudioRingProducer,
    metrics: RouteMetricsStore,
    reducer_factory: F,
) -> Result<PipelineWorkerHandle, WorkerStartError>
where
    R: NoiseReducer + 'static,
    F: FnOnce() -> Result<R, DenoiseError> + Send + 'static,
{
    let (command_tx, command_rx) = sync_channel(DEFAULT_CONTROL_CAPACITY);
    let (ready_tx, ready_rx) = sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let join = thread::Builder::new()
        .name("openkrisp-dsp".to_owned())
        .spawn(move || {
            let mut reducer = match reducer_factory() {
                Ok(reducer) => reducer,
                Err(error) => {
                    let error = WorkerError::Denoise(error);
                    let _ = ready_tx.send(Err(error.clone()));
                    return Err(error);
                }
            };
            let descriptor = reducer.descriptor().clone();
            let mut graph = match PipelineGraph::new(
                &config,
                &descriptor,
                &metrics,
                output.capacity_frames(),
            ) {
                Ok(graph) => graph,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.clone()));
                    return Err(error);
                }
            };
            if ready_tx.send(Ok(descriptor)).is_err() {
                return Ok(());
            }
            graph.run(
                &mut reducer,
                input,
                output,
                &command_rx,
                &worker_cancelled,
                &metrics,
            )
        })
        .map_err(|error| WorkerStartError::ThreadSpawn(error.to_string()))?;
    let thread = join.thread().clone();

    match ready_rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(descriptor)) => Ok(PipelineWorkerHandle {
            commands: command_tx,
            cancelled,
            thread,
            join: Some(join),
            descriptor,
        }),
        Ok(Err(error)) => {
            let _ = join.join();
            Err(WorkerStartError::Initialization(error))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            cancelled.store(true, Ordering::Release);
            thread.unpark();
            let _ = join.join();
            Err(WorkerStartError::ReadinessTimeout)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let _ = join.join();
            Err(WorkerStartError::ReadinessDisconnected)
        }
    }
}

fn join_worker(join: &mut Option<JoinHandle<Result<(), WorkerError>>>) -> Result<(), WorkerError> {
    match join.take() {
        Some(join) => join.join().map_err(|_| WorkerError::Panicked)?,
        None => Ok(()),
    }
}

struct PipelineGraph {
    input_stale_target_frames: usize,
    input_channels: usize,
    output_channels: usize,
    input_mapper: ChannelMapper,
    input_resampler: StreamingResampler,
    frame_assembler: ModelFrameAssembler,
    dry_delay: FixedDryDelay,
    crossfade: EqualPowerCrossfade,
    limiter: SafetyLimiter,
    input_meter: LevelMeter,
    output_meter: LevelMeter,
    output_resampler: StreamingResampler,
    output_mapper: ChannelMapper,
    input_scratch: Vec<f32>,
    mapped_input: Vec<f32>,
    resampled_input: Vec<f32>,
    wet_model_frame: Vec<f32>,
    dry_model_frame: Vec<f32>,
    mixed_model_frame: Vec<f32>,
    resampled_output: Vec<f32>,
    mapped_output: Vec<f32>,
    bypassed: bool,
    denoise_enabled: bool,
    model_deadline: Duration,
    inference_history: InferenceHistory,
    drift_controller: DriftController,
    overload_guard: OverloadGuard,
}

impl PipelineGraph {
    fn new(
        config: &PipelineWorkerConfig,
        descriptor: &NoiseReducerDescriptor,
        metrics: &RouteMetricsStore,
        output_capacity_frames: usize,
    ) -> Result<Self, WorkerError> {
        if descriptor.channels.get() != 1 {
            return Err(WorkerError::UnsupportedModelChannels);
        }
        if config.output_target_fill_frames == 0 {
            return Err(WorkerError::ZeroOutputFillTarget);
        }
        if config.output_target_fill_frames > output_capacity_frames {
            return Err(WorkerError::OutputFillTargetExceedsCapacity {
                target_frames: config.output_target_fill_frames,
                capacity_frames: output_capacity_frames,
            });
        }
        let input_layout = ChannelLayout::from_channels(config.input_format.channels);
        let model_layout = ChannelLayout::Mono;
        let output_layout = ChannelLayout::from_channels(config.output_format.channels);
        let input_mapper = ChannelMapper::new(input_layout, model_layout)?;
        let output_mapper = ChannelMapper::new(model_layout, output_layout)?;
        let input_chunk_frames = config.input_format.sample_rate_hz.get().div_ceil(100) as usize;
        let input_resampler = StreamingResampler::new(
            config.input_format.sample_rate_hz,
            descriptor.sample_rate_hz,
            descriptor.channels,
            input_chunk_frames,
        )?;
        let output_resampler = StreamingResampler::new_adjustable(
            descriptor.sample_rate_hz,
            config.output_format.sample_rate_hz,
            descriptor.channels,
            descriptor.frame_samples_per_channel,
        )?;
        let input_resampler_output_frames_max = input_resampler.output_frames_max();
        let output_resampler_output_frames_max = output_resampler.output_frames_max();
        let model_samples = descriptor.frame_samples()?;
        let crossfade_frames =
            ((u64::from(descriptor.sample_rate_hz.get()) * u64::from(config.crossfade_ms)) / 1_000)
                .max(1) as usize;
        let initial_target = if config.initially_bypassed || !config.denoise_enabled {
            audio_dsp::CrossfadeTarget::Dry
        } else {
            audio_dsp::CrossfadeTarget::Wet
        };
        let fixed_latency = frames_duration(
            descriptor
                .algorithmic_latency_samples
                .saturating_add(input_resampler.output_delay_frames()),
            descriptor.sample_rate_hz.get(),
        )
        .saturating_add(frames_duration(
            output_resampler.output_delay_frames(),
            config.output_format.sample_rate_hz.get(),
        ));
        metrics.set_fixed_latency(fixed_latency);
        let input_scratch_samples = input_chunk_frames
            .checked_mul(config.input_format.channels.as_usize())
            .ok_or(DspError::ResamplerSizeOverflow)?;
        let mapped_input_samples = input_chunk_frames
            .checked_mul(descriptor.channels.as_usize())
            .ok_or(DspError::ResamplerSizeOverflow)?;
        let mapped_output_samples = output_resampler_output_frames_max
            .checked_mul(config.output_format.channels.as_usize())
            .ok_or(DspError::ResamplerSizeOverflow)?;
        Ok(Self {
            input_stale_target_frames: config.input_stale_target_frames,
            input_channels: config.input_format.channels.as_usize(),
            output_channels: config.output_format.channels.as_usize(),
            input_mapper,
            input_resampler,
            frame_assembler: ModelFrameAssembler::new(
                descriptor.channels,
                descriptor.frame_samples_per_channel,
            )?,
            dry_delay: FixedDryDelay::new(
                descriptor.channels,
                descriptor.algorithmic_latency_samples,
            )?,
            crossfade: EqualPowerCrossfade::new(
                descriptor.channels,
                crossfade_frames,
                initial_target,
            )?,
            limiter: SafetyLimiter::new(0.98)?,
            input_meter: LevelMeter::new(0.85)?,
            output_meter: LevelMeter::new(0.85)?,
            resampled_input: vec![0.0; input_resampler_output_frames_max],
            resampled_output: vec![0.0; output_resampler_output_frames_max],
            input_scratch: vec![0.0; input_scratch_samples],
            mapped_input: vec![0.0; mapped_input_samples],
            wet_model_frame: vec![0.0; model_samples],
            dry_model_frame: vec![0.0; model_samples],
            mixed_model_frame: vec![0.0; model_samples],
            mapped_output: vec![0.0; mapped_output_samples],
            output_resampler,
            output_mapper,
            bypassed: config.initially_bypassed,
            denoise_enabled: config.denoise_enabled,
            model_deadline: frames_duration(
                descriptor.frame_samples_per_channel,
                descriptor.sample_rate_hz.get(),
            ),
            inference_history: InferenceHistory::default(),
            drift_controller: DriftController::new(DriftControllerConfig::for_target(
                config.output_target_fill_frames,
            )),
            overload_guard: OverloadGuard::default(),
        })
    }

    fn run<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        mut input: AudioRingConsumer,
        mut output: AudioRingProducer,
        commands: &Receiver<WorkerCommand>,
        cancelled: &AtomicBool,
        metrics: &RouteMetricsStore,
    ) -> Result<(), WorkerError> {
        let mut input_discontinuities = metrics.input_discontinuities();
        let mut output_discontinuities = metrics.output_discontinuities();
        while !cancelled.load(Ordering::Acquire) {
            self.apply_commands(reducer, commands, cancelled, metrics);
            if cancelled.load(Ordering::Acquire) {
                break;
            }

            input.trim_oldest_to(self.input_stale_target_frames);
            let next_input_discontinuities = metrics.input_discontinuities();
            let next_output_discontinuities = metrics.output_discontinuities();
            if next_input_discontinuities != input_discontinuities
                || next_output_discontinuities != output_discontinuities
            {
                self.reset_after_discontinuity(reducer, metrics);
                input_discontinuities = next_input_discontinuities;
                output_discontinuities = next_output_discontinuities;
            }
            let read = input.read(&mut self.input_scratch)?;
            if read.read_frames == 0 {
                thread::park_timeout(IDLE_PARK);
                continue;
            }
            let input_samples = read.read_frames * self.input_channels;
            self.process_input(reducer, &mut output, input_samples, metrics)?;
        }
        Ok(())
    }

    fn apply_commands<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        commands: &Receiver<WorkerCommand>,
        cancelled: &AtomicBool,
        metrics: &RouteMetricsStore,
    ) {
        loop {
            match commands.try_recv() {
                Ok(WorkerCommand::UpdateSettings(settings)) => {
                    self.denoise_enabled = settings.enabled;
                    if reducer.update_settings(&settings).is_err() {
                        metrics.record_model_error();
                        self.bypassed = true;
                    }
                }
                Ok(WorkerCommand::SetBypass(bypassed)) => self.bypassed = bypassed,
                Ok(WorkerCommand::Shutdown) => {
                    cancelled.store(true, Ordering::Release);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    cancelled.store(true, Ordering::Release);
                    break;
                }
            }
        }
    }

    fn process_input<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        output: &mut AudioRingProducer,
        input_samples: usize,
        metrics: &RouteMetricsStore,
    ) -> Result<(), WorkerError> {
        let input = &self.input_scratch[..input_samples];
        let mapped_frames = self.input_mapper.process(input, &mut self.mapped_input)?;
        let input_level = self
            .input_meter
            .process(&self.mapped_input[..mapped_frames]);
        metrics.record_input_level(input_level.peak, input_level.rms);

        let mut input_offset = 0;
        while input_offset < mapped_frames || self.input_resampler.pending_output_frames() > 0 {
            let progress = self.input_resampler.process(
                &self.mapped_input[input_offset..mapped_frames],
                &mut self.resampled_input,
            )?;
            if progress.input_frames_consumed == 0 && progress.output_frames_written == 0 {
                return Err(WorkerError::ResamplerStalled);
            }
            input_offset += progress.input_frames_consumed;
            if progress.output_frames_written > 0 {
                self.process_resampled_model_input(
                    reducer,
                    output,
                    progress.output_frames_written,
                    metrics,
                )?;
            }
        }
        Ok(())
    }

    fn process_resampled_model_input<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        output: &mut AudioRingProducer,
        frames: usize,
        metrics: &RouteMetricsStore,
    ) -> Result<(), WorkerError> {
        let mut frame_error = None;
        let samples = &self.resampled_input[..frames];
        let dry_delay = &mut self.dry_delay;
        let crossfade = &mut self.crossfade;
        let limiter = self.limiter;
        let output_meter = &mut self.output_meter;
        let output_resampler = &mut self.output_resampler;
        let output_mapper = &self.output_mapper;
        let wet_model_frame = &mut self.wet_model_frame;
        let dry_model_frame = &mut self.dry_model_frame;
        let mixed_model_frame = &mut self.mixed_model_frame;
        let resampled_output = &mut self.resampled_output;
        let mapped_output = &mut self.mapped_output;
        let output_channels = self.output_channels;
        let bypassed = self.bypassed;
        let denoise_enabled = self.denoise_enabled;
        let model_deadline = self.model_deadline;
        let inference_history = &mut self.inference_history;
        let drift_controller = &mut self.drift_controller;
        let overload_guard = &mut self.overload_guard;

        self.frame_assembler.push(samples, |model_input| {
            if frame_error.is_some() {
                return;
            }
            frame_error = process_model_frame(
                reducer,
                output,
                metrics,
                model_input,
                dry_delay,
                crossfade,
                limiter,
                output_meter,
                output_resampler,
                output_mapper,
                wet_model_frame,
                dry_model_frame,
                mixed_model_frame,
                resampled_output,
                mapped_output,
                output_channels,
                bypassed,
                denoise_enabled,
                model_deadline,
                inference_history,
                drift_controller,
                overload_guard,
            )
            .err();
        })?;
        match frame_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn reset_after_discontinuity<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        metrics: &RouteMetricsStore,
    ) {
        self.input_resampler.reset();
        self.frame_assembler.reset();
        self.dry_delay.reset();
        self.output_resampler.reset();
        self.drift_controller.reset();
        metrics.record_drift_correction(0);
        self.crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
        if reducer.reset().is_ok() {
            metrics.record_model_reset();
        } else {
            metrics.record_model_error();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_model_frame<R: NoiseReducer>(
    reducer: &mut R,
    output: &mut AudioRingProducer,
    metrics: &RouteMetricsStore,
    model_input: &[f32],
    dry_delay: &mut FixedDryDelay,
    crossfade: &mut EqualPowerCrossfade,
    limiter: SafetyLimiter,
    output_meter: &mut LevelMeter,
    output_resampler: &mut StreamingResampler,
    output_mapper: &ChannelMapper,
    wet_model_frame: &mut [f32],
    dry_model_frame: &mut [f32],
    mixed_model_frame: &mut [f32],
    resampled_output: &mut [f32],
    mapped_output: &mut [f32],
    output_channels: usize,
    bypassed: bool,
    denoise_enabled: bool,
    model_deadline: Duration,
    inference_history: &mut InferenceHistory,
    drift_controller: &mut DriftController,
    overload_guard: &mut OverloadGuard,
) -> Result<(), WorkerError> {
    dry_delay.process(model_input, dry_model_frame)?;
    if overload_guard.should_process_model() {
        let probing_degraded_route = overload_guard.is_degraded();
        let started = Instant::now();
        // Skipped frames make recurrent model state stale. Reset before every sparse probe so the
        // reducer never treats non-contiguous audio as one continuous sequence.
        let probe_reset_failed = if probing_degraded_route {
            if reducer.reset().is_ok() {
                metrics.record_model_reset();
                false
            } else {
                metrics.record_model_error();
                true
            }
        } else {
            false
        };
        let model_result = reducer.process_planar(model_input, wet_model_frame);
        let elapsed = started.elapsed();
        let model_call_failed = model_result.is_err();
        let model_failed = probe_reset_failed || model_call_failed;
        let (p95_us, p99_us) = inference_history.record(elapsed);
        metrics.record_inference(elapsed, model_deadline, p95_us, p99_us);
        match overload_guard.record_attempt(model_failed || elapsed > model_deadline) {
            OverloadTransition::EnteredDegraded => metrics.set_degraded(true),
            OverloadTransition::ExitedDegraded => metrics.set_degraded(false),
            OverloadTransition::None => {}
        }

        if model_failed {
            if model_call_failed {
                metrics.record_model_error();
            }
            wet_model_frame.copy_from_slice(dry_model_frame);
            crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
            if reducer.reset().is_ok() {
                metrics.record_model_reset();
            } else {
                metrics.record_model_error();
            }
        } else if bypassed || !denoise_enabled || overload_guard.is_degraded() {
            crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
        } else {
            crossfade.set_target(audio_dsp::CrossfadeTarget::Wet);
        }
    } else {
        metrics.record_skipped_inference();
        wet_model_frame.copy_from_slice(dry_model_frame);
        crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
    }

    crossfade.process(dry_model_frame, wet_model_frame, mixed_model_frame)?;
    limiter.process_in_place(mixed_model_frame);
    let level = output_meter.process(mixed_model_frame);
    metrics.record_output_level(level.peak, level.rms);

    let mut input_offset = 0;
    while input_offset < mixed_model_frame.len() || output_resampler.pending_output_frames() > 0 {
        let progress =
            output_resampler.process(&mixed_model_frame[input_offset..], resampled_output)?;
        if progress.input_frames_consumed == 0 && progress.output_frames_written == 0 {
            return Err(WorkerError::ResamplerStalled);
        }
        input_offset += progress.input_frames_consumed;
        if progress.output_frames_written > 0 {
            let mapped_samples = progress.output_frames_written * output_channels;
            output_mapper.process(
                &resampled_output[..progress.output_frames_written],
                &mut mapped_output[..mapped_samples],
            )?;
            output.write(&mapped_output[..mapped_samples])?;
        }
    }
    // Sampling after this frame is queued makes the configured target describe the post-produce
    // occupancy. The new ratio is ramped over the next model frame.
    let correction_ppm = drift_controller.update(metrics.output_fill_frames());
    output_resampler.set_correction_ppm(correction_ppm)?;
    metrics.record_drift_correction(correction_ppm);
    Ok(())
}

#[derive(Clone)]
struct InferenceHistory {
    samples_us: [u64; INFERENCE_WINDOW],
    len: usize,
    cursor: usize,
}

impl Default for InferenceHistory {
    fn default() -> Self {
        Self {
            samples_us: [0; INFERENCE_WINDOW],
            len: 0,
            cursor: 0,
        }
    }
}

impl InferenceHistory {
    fn record(&mut self, elapsed: Duration) -> (u64, u64) {
        self.samples_us[self.cursor] = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.cursor = (self.cursor + 1) % INFERENCE_WINDOW;
        self.len = (self.len + 1).min(INFERENCE_WINDOW);
        let mut ordered = self.samples_us;
        ordered[..self.len].sort_unstable();
        (
            percentile(&ordered[..self.len], 95),
            percentile(&ordered[..self.len], 99),
        )
    }
}

fn percentile(ordered: &[u64], percentile: usize) -> u64 {
    if ordered.is_empty() {
        return 0;
    }
    let index = (ordered.len() - 1) * percentile / 100;
    ordered[index]
}

fn frames_duration(frames: usize, sample_rate_hz: u32) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(sample_rate_hz))
}

#[cfg(test)]
mod tests {
    use std::{
        rc::Rc,
        sync::{
            atomic::AtomicU64,
            mpsc::{Receiver, SyncSender},
        },
        time::Duration,
    };

    use audio_protocol::{ChannelCount, SampleFormat, SampleRateHz};
    use denoise_core::{
        FakeFailureMode, FakeNoiseReducer, FakeNoiseReducerConfig, NoiseReductionStats,
    };

    use super::*;
    use crate::bounded_audio_ring;

    struct LocalReducer {
        inner: FakeNoiseReducer,
        _not_send: Rc<()>,
    }

    impl NoiseReducer for LocalReducer {
        fn descriptor(&self) -> &NoiseReducerDescriptor {
            self.inner.descriptor()
        }

        fn process_planar(
            &mut self,
            input: &[f32],
            output: &mut [f32],
        ) -> Result<denoise_core::NoiseReductionStats, DenoiseError> {
            self.inner.process_planar(input, output)
        }

        fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
            self.inner.update_settings(settings)
        }

        fn reset(&mut self) -> Result<(), DenoiseError> {
            self.inner.reset()
        }
    }

    struct GateReducer {
        inner: FakeNoiseReducer,
        entered: Option<SyncSender<()>>,
        release: Receiver<()>,
    }

    impl NoiseReducer for GateReducer {
        fn descriptor(&self) -> &NoiseReducerDescriptor {
            self.inner.descriptor()
        }

        fn process_planar(
            &mut self,
            input: &[f32],
            output: &mut [f32],
        ) -> Result<NoiseReductionStats, DenoiseError> {
            if let Some(entered) = self.entered.take() {
                let _ = entered.send(());
                self.release.recv().map_err(|_| {
                    DenoiseError::InferenceFailed("test gate disconnected".to_owned())
                })?;
            }
            self.inner.process_planar(input, output)
        }

        fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
            self.inner.update_settings(settings)
        }

        fn reset(&mut self) -> Result<(), DenoiseError> {
            self.inner.reset()
        }
    }

    struct SlowReducer {
        inner: FakeNoiseReducer,
        calls: Arc<AtomicU64>,
        delay: Duration,
        slow_calls: u64,
    }

    impl NoiseReducer for SlowReducer {
        fn descriptor(&self) -> &NoiseReducerDescriptor {
            self.inner.descriptor()
        }

        fn process_planar(
            &mut self,
            input: &[f32],
            output: &mut [f32],
        ) -> Result<NoiseReductionStats, DenoiseError> {
            let call = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
            if call <= self.slow_calls {
                thread::sleep(self.delay);
            }
            self.inner.process_planar(input, output)
        }

        fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
            self.inner.update_settings(settings)
        }

        fn reset(&mut self) -> Result<(), DenoiseError> {
            self.inner.reset()
        }
    }

    fn format(channels: u16) -> AudioFormat {
        format_at(48_000, channels)
    }

    fn format_at(sample_rate_hz: u32, channels: u16) -> AudioFormat {
        AudioFormat {
            sample_rate_hz: SampleRateHz::new(sample_rate_hz).expect("valid rate"),
            channels: ChannelCount::new(channels).expect("valid channels"),
            sample_format: SampleFormat::F32,
            buffer_size_frames: None,
        }
    }

    fn fake() -> FakeNoiseReducer {
        fake_with(0, FakeFailureMode::Never)
    }

    fn fake_with(
        algorithmic_latency_samples: usize,
        failure_mode: FakeFailureMode,
    ) -> FakeNoiseReducer {
        FakeNoiseReducer::new(FakeNoiseReducerConfig {
            descriptor: NoiseReducerDescriptor {
                model_id: "worker-fake".to_owned(),
                sample_rate_hz: SampleRateHz::new(48_000).expect("valid rate"),
                channels: ChannelCount::new(1).expect("valid channels"),
                frame_samples_per_channel: 480,
                algorithmic_latency_samples,
            },
            settings: DenoiseSettings::default(),
            enabled_gain: 0.5,
            processing_delay: Duration::ZERO,
            failure_mode,
            stats: NoiseReductionStats::default(),
        })
        .expect("valid fake")
    }

    fn wait_for_output(output: &AudioRingConsumer, target_frames: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while output.metrics().snapshot().fill_frames < target_frames && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        output.metrics().snapshot().fill_frames >= target_frames
    }

    fn wait_for_drained_input(input: &AudioRingProducer, output: &AudioRingConsumer) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut previous_output_fill = usize::MAX;
        let mut stable_polls = 0;
        while Instant::now() < deadline {
            let input_fill = input.metrics().snapshot().fill_frames;
            let output_fill = output.metrics().snapshot().fill_frames;
            if input_fill == 0 && output_fill == previous_output_fill {
                stable_polls += 1;
                if stable_polls == 20 {
                    return true;
                }
            } else {
                stable_polls = 0;
            }
            previous_output_fill = output_fill;
            thread::sleep(Duration::from_millis(1));
        }
        false
    }

    #[test]
    fn non_send_reducer_is_constructed_processed_and_dropped_on_worker() {
        let channels = ChannelCount::new(1).expect("valid channels");
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(4_800, channels).expect("input ring");
        let (output_producer, mut output_consumer) =
            bounded_audio_ring(4_800, channels).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(1),
            format(1),
        );
        let mut config = PipelineWorkerConfig::new(format(1), format(1), 4_800);
        config.crossfade_ms = 0;
        let mut worker = spawn_pipeline_worker(
            config,
            input_consumer,
            output_producer,
            metrics.clone(),
            || {
                Ok(LocalReducer {
                    inner: fake(),
                    _not_send: Rc::new(()),
                })
            },
        )
        .expect("worker starts");

        input_producer.write(&[0.5; 960]).expect("input write");
        worker.wake();
        let deadline = Instant::now() + Duration::from_secs(2);
        while output_consumer.metrics().snapshot().fill_frames < 950 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        let output_frames = output_consumer.metrics().fill_frames();
        let mut output = vec![0.0; output_frames];
        let read = output_consumer.read(&mut output).expect("output read");
        assert!(read.read_frames >= 950);
        assert!(
            output[output.len() / 2..]
                .iter()
                .all(|sample| (*sample - 0.25).abs() < 1.0e-3)
        );
        assert!(metrics.snapshot().inference_last_us < 10_000);
        worker.shutdown().expect("worker joins");
        assert!(worker.is_finished());
    }

    #[test]
    fn worker_maps_stereo_and_resamples_between_device_rates() {
        let stereo = ChannelCount::new(2).expect("valid channels");
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(44_100, stereo).expect("input ring");
        let (output_producer, mut output_consumer) =
            bounded_audio_ring(100_000, stereo).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format_at(44_100, 2),
            format_at(96_000, 2),
        );
        let mut config =
            PipelineWorkerConfig::new(format_at(44_100, 2), format_at(96_000, 2), 44_100);
        config.crossfade_ms = 0;
        let mut worker =
            spawn_pipeline_worker(config, input_consumer, output_producer, metrics, || {
                Ok(fake())
            })
            .expect("worker starts");

        let input = [0.6_f32, 0.2]
            .into_iter()
            .cycle()
            .take(44_100 * 2)
            .collect::<Vec<_>>();
        let write = input_producer.write(&input).expect("input write");
        assert_eq!(write.dropped_frames, 0);
        worker.wake();
        // A live pipeline retains at most one incomplete 10 ms model frame until more input
        // arrives, so one finite second need not immediately expose every nominal output frame.
        let completed = wait_for_drained_input(&input_producer, &output_consumer);
        worker.shutdown().expect("worker joins");

        let output_frames = output_consumer.metrics().snapshot().fill_frames;
        let mut output = vec![0.0; output_frames * 2];
        let read = output_consumer.read(&mut output).expect("output read");
        assert!(completed, "worker produced only {output_frames} frames");
        assert_eq!(read.read_frames, output_frames);
        assert!(output_frames.abs_diff(96_000) <= 1_100);
        assert!(output.iter().all(|sample| sample.is_finite()));
        let (stereo_frames, remainder) = output.as_chunks::<2>();
        assert!(remainder.is_empty());
        assert!(
            stereo_frames
                .iter()
                .all(|frame| frame[0].to_bits() == frame[1].to_bits())
        );
        let steady_sample = output[(output_frames / 2) * 2];
        assert!((steady_sample - 0.2).abs() < 0.01, "{steady_sample}");
    }

    #[test]
    fn model_failure_uses_the_latency_aligned_dry_frame() {
        let mono = ChannelCount::new(1).expect("valid channels");
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(1_920, mono).expect("input ring");
        let (output_producer, mut output_consumer) =
            bounded_audio_ring(1_920, mono).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(1),
            format(1),
        );
        let mut config = PipelineWorkerConfig::new(format(1), format(1), 1_920);
        config.crossfade_ms = 0;
        let mut worker = spawn_pipeline_worker(
            config,
            input_consumer,
            output_producer,
            metrics.clone(),
            || Ok(fake_with(480, FakeFailureMode::Always)),
        )
        .expect("worker starts");

        input_producer.write(&[0.4; 960]).expect("input write");
        worker.wake();
        let completed = wait_for_output(&output_consumer, 950);
        worker.shutdown().expect("worker joins");
        let output_frames = output_consumer.metrics().fill_frames();
        let mut output = vec![0.0; output_frames];
        let read = output_consumer.read(&mut output).expect("output read");
        let snapshot = metrics.snapshot();

        assert!(completed);
        assert_eq!(read.read_frames, output_frames);
        assert!(output[..480].iter().all(|sample| sample.abs() < 1.0e-6));
        assert!(
            output[output.len() - 240..]
                .iter()
                .all(|sample| (*sample - 0.4).abs() < 1.0e-3)
        );
        assert_eq!(metrics.model_errors(), 2);
        assert_eq!(snapshot.model_resets, 2);
    }

    #[test]
    fn input_overflow_is_detected_and_resets_temporal_state() {
        let mono = ChannelCount::new(1).expect("valid channels");
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(1_440, mono).expect("input ring");
        let (output_producer, output_consumer) =
            bounded_audio_ring(4_800, mono).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(1),
            format(1),
        );
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
        let mut config = PipelineWorkerConfig::new(format(1), format(1), 1_440);
        config.crossfade_ms = 0;
        let mut worker = spawn_pipeline_worker(
            config,
            input_consumer,
            output_producer,
            metrics.clone(),
            move || {
                Ok(GateReducer {
                    inner: fake(),
                    entered: Some(entered_tx),
                    release: release_rx,
                })
            },
        )
        .expect("worker starts");

        input_producer.write(&[0.25; 480]).expect("first input");
        worker.wake();
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("worker enters model call");
        let overflow = input_producer
            .write(&[0.5; 1_920])
            .expect("overflowing input remains non-blocking");
        assert_eq!(overflow.dropped_frames, 480);
        release_tx.send(()).expect("release worker");
        worker.wake();
        assert!(wait_for_drained_input(&input_producer, &output_consumer));
        worker.shutdown().expect("worker joins");

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.input_overruns, 1);
        assert_eq!(snapshot.discontinuities, 1);
        assert_eq!(snapshot.model_resets, 1);
        assert!(snapshot.drift_correction_ppm.abs() <= 30);
    }

    #[test]
    fn one_cpu_spike_records_a_miss_without_degrading() {
        let mono = ChannelCount::new(1).expect("valid channels");
        let model_frames = 16;
        let input_frames = model_frames * 480;
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(input_frames, mono).expect("input ring");
        let (output_producer, output_consumer) =
            bounded_audio_ring(input_frames * 2, mono).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(1),
            format(1),
        );
        let calls = Arc::new(AtomicU64::new(0));
        let reducer_calls = calls.clone();
        let mut config = PipelineWorkerConfig::new(format(1), format(1), input_frames);
        config.crossfade_ms = 0;
        let mut worker = spawn_pipeline_worker(
            config,
            input_consumer,
            output_producer,
            metrics.clone(),
            move || {
                Ok(SlowReducer {
                    inner: fake(),
                    calls: reducer_calls,
                    delay: Duration::from_millis(12),
                    slow_calls: 1,
                })
            },
        )
        .expect("worker starts");

        input_producer
            .write(&vec![0.4; input_frames])
            .expect("input write");
        worker.wake();
        assert!(wait_for_drained_input(&input_producer, &output_consumer));
        worker.shutdown().expect("worker joins");

        let snapshot = metrics.snapshot();
        assert!(snapshot.missed_deadlines >= 1);
        assert!(!snapshot.degraded);
        assert_eq!(snapshot.overload_events, 0);
        assert_eq!(snapshot.skipped_inferences, 0);
        assert_eq!(calls.load(Ordering::Relaxed), model_frames as u64);
    }

    #[test]
    fn sustained_deadline_misses_enter_aligned_degraded_bypass() {
        let mono = ChannelCount::new(1).expect("valid channels");
        let model_frames = 32;
        let input_frames = model_frames * 480;
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(input_frames, mono).expect("input ring");
        let (output_producer, mut output_consumer) =
            bounded_audio_ring(input_frames * 2, mono).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(1),
            format(1),
        );
        let calls = Arc::new(AtomicU64::new(0));
        let reducer_calls = calls.clone();
        let mut config = PipelineWorkerConfig::new(format(1), format(1), input_frames);
        config.crossfade_ms = 0;
        let mut worker = spawn_pipeline_worker(
            config,
            input_consumer,
            output_producer,
            metrics.clone(),
            move || {
                Ok(SlowReducer {
                    inner: fake_with(480, FakeFailureMode::Never),
                    calls: reducer_calls,
                    delay: Duration::from_millis(12),
                    slow_calls: u64::MAX,
                })
            },
        )
        .expect("worker starts");

        input_producer
            .write(&vec![0.4; input_frames])
            .expect("input write");
        worker.wake();
        assert!(wait_for_drained_input(&input_producer, &output_consumer));
        worker.shutdown().expect("worker joins");

        let snapshot = metrics.snapshot();
        let attempted_inferences = calls.load(Ordering::Relaxed);
        assert!(snapshot.degraded);
        assert_eq!(snapshot.overload_events, 1);
        assert!(snapshot.missed_deadlines >= 8);
        assert!(snapshot.skipped_inferences > 0);
        assert_eq!(snapshot.model_resets, 3);
        assert!(attempted_inferences < model_frames as u64 / 2);
        assert!(snapshot.drift_correction_ppm.abs() <= 1_000);

        let output_frames = output_consumer.metrics().fill_frames();
        let mut output = vec![0.0; output_frames];
        output_consumer.read(&mut output).expect("output read");
        assert!(
            output[output.len() - 240..]
                .iter()
                .all(|sample| (*sample - 0.4).abs() < 1.0e-3)
        );
    }

    #[test]
    fn output_fill_target_cannot_exceed_the_bounded_ring() {
        let mono = ChannelCount::new(1).expect("valid channels");
        let (_input_producer, input_consumer) = bounded_audio_ring(960, mono).expect("input ring");
        let (output_producer, _output_consumer) =
            bounded_audio_ring(960, mono).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(1),
            format(1),
        );
        let mut config = PipelineWorkerConfig::new(format(1), format(1), 960);
        config.output_target_fill_frames = 961;

        let result =
            spawn_pipeline_worker(config, input_consumer, output_producer, metrics, || {
                Ok(fake())
            });
        assert!(matches!(
            result,
            Err(WorkerStartError::Initialization(
                WorkerError::OutputFillTargetExceedsCapacity {
                    target_frames: 961,
                    capacity_frames: 960
                }
            ))
        ));
    }

    #[test]
    fn worker_control_queue_is_bounded() {
        let mono = ChannelCount::new(1).expect("valid channels");
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(960, mono).expect("input ring");
        let (output_producer, _output_consumer) =
            bounded_audio_ring(960, mono).expect("output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(1),
            format(1),
        );
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
        let mut config = PipelineWorkerConfig::new(format(1), format(1), 960);
        config.output_target_fill_frames = 960;
        let mut worker = spawn_pipeline_worker(
            config,
            input_consumer,
            output_producer,
            metrics,
            move || {
                Ok(GateReducer {
                    inner: fake(),
                    entered: Some(entered_tx),
                    release: release_rx,
                })
            },
        )
        .expect("worker starts");

        input_producer.write(&[0.25; 480]).expect("input write");
        worker.wake();
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("worker enters model call");
        let accepted = (0..DEFAULT_CONTROL_CAPACITY)
            .map(|index| worker.set_bypass(index.is_multiple_of(2)))
            .collect::<Vec<_>>();
        let overflow = worker.set_bypass(true);
        release_tx.send(()).expect("release worker");
        worker.shutdown().expect("worker joins");

        assert!(accepted.iter().all(Result::is_ok));
        assert_eq!(overflow, Err(WorkerCommandError::QueueFull));
    }
}
