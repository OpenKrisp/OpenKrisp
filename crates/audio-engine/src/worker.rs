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

#[cfg(test)]
#[path = "worker/incoming_tests.rs"]
mod incoming_tests;

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
    /// Incoming routes skip inference in bypass and yield to microphone pressure.
    pub incoming: bool,
    pub priority_pressure: Arc<AtomicBool>,
    #[cfg(feature = "aec-prototype")]
    pub(crate) aec: Option<crate::AecDiagnostics>,
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
            incoming: false,
            priority_pressure: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "aec-prototype")]
            aec: None,
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
    #[error("the worker supports mono/stereo reducers; incoming stereo requires stereo output")]
    UnsupportedModelChannels,
    #[cfg(feature = "aec-prototype")]
    #[error("AEC requires timestamped microphone capture and a 48 kHz mono, 480-sample model")]
    InvalidAecConfiguration,
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
    #[error("DSP worker startup was cancelled")]
    Cancelled,
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
    #[cfg(feature = "aec-prototype")]
    pub(crate) aec_reference: SyncSender<audio_cpal::AudioTapReader>,
    #[cfg(feature = "aec-prototype")]
    pub(crate) aec_returned: Receiver<audio_cpal::AudioTapReader>,
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
    spawn_pipeline_worker_controlled(
        config,
        input.into(),
        output,
        metrics,
        reducer_factory,
        &AtomicBool::new(false),
    )
}

pub(crate) enum WorkerInput {
    Samples(AudioRingConsumer),
    #[cfg(feature = "aec-prototype")]
    Timed(audio_cpal::AudioTapReader),
}
impl From<AudioRingConsumer> for WorkerInput {
    fn from(input: AudioRingConsumer) -> Self {
        Self::Samples(input)
    }
}

pub(crate) fn spawn_pipeline_worker_controlled<R, F>(
    config: PipelineWorkerConfig,
    input: WorkerInput,
    output: AudioRingProducer,
    metrics: RouteMetricsStore,
    reducer_factory: F,
    startup_cancelled: &AtomicBool,
) -> Result<PipelineWorkerHandle, WorkerStartError>
where
    R: NoiseReducer + 'static,
    F: FnOnce() -> Result<R, DenoiseError> + Send + 'static,
{
    if startup_cancelled.load(Ordering::Acquire) {
        return Err(WorkerStartError::Cancelled);
    }
    let (command_tx, command_rx) = sync_channel(DEFAULT_CONTROL_CAPACITY);
    let (ready_tx, ready_rx) = sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    #[cfg(feature = "aec-prototype")]
    let (aec_tx, aec_rx) = sync_channel(1);
    #[cfg(feature = "aec-prototype")]
    let (return_tx, return_rx) = sync_channel(1);
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
            #[cfg(feature = "aec-prototype")]
            if let Some(diagnostics) = config.aec {
                let result = match &input {
                    WorkerInput::Timed(reader) => crate::aec::AecStage::new(
                        diagnostics,
                        reader.control(),
                        graph.input_resampler.output_delay_frames(),
                        aec_rx,
                        return_tx,
                    )
                    .map_err(WorkerError::Dsp),
                    WorkerInput::Samples(_) => Err(WorkerError::InvalidAecConfiguration),
                };
                match result {
                    Ok(stage) => graph.model.aec = Some(stage),
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.clone()));
                        return Err(error);
                    }
                }
            }
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

    let deadline = Instant::now() + READY_TIMEOUT;
    let readiness = loop {
        if startup_cancelled.load(Ordering::Acquire) {
            cancelled.store(true, Ordering::Release);
            thread.unpark();
            // A native model construction already in progress must finish on its owner thread.
            let _ = join.join();
            return Err(WorkerStartError::Cancelled);
        }
        match ready_rx.recv_timeout(Duration::from_millis(5)) {
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            result => break result,
        }
    };
    match readiness {
        Ok(Ok(descriptor)) => Ok(PipelineWorkerHandle {
            commands: command_tx,
            cancelled,
            thread,
            join: Some(join),
            descriptor,
            #[cfg(feature = "aec-prototype")]
            aec_reference: aec_tx,
            #[cfg(feature = "aec-prototype")]
            aec_returned: return_rx,
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
    model_channels: usize,
    input_mapper: ChannelMapper,
    input_resampler: StreamingResampler,
    frame_assembler: ModelFrameAssembler,
    input_meter: LevelMeter,
    input_scratch: Vec<f32>,
    mapped_input: Vec<f32>,
    resampled_input: Vec<f32>,
    model: ModelStage,
}

/// Model-rate processing and the final device-rate output stage own their reusable storage.
struct ModelStage {
    #[cfg(feature = "aec-prototype")]
    aec: Option<crate::aec::AecStage>,
    dry_delay: FixedDryDelay,
    crossfade: EqualPowerCrossfade,
    limiter: SafetyLimiter,
    output_meter: LevelMeter,
    output_resampler: StreamingResampler,
    output_mapper: ChannelMapper,
    wet_model_frame: Vec<f32>,
    planar_input: Vec<f32>,
    planar_output: Vec<f32>,
    model_channels: usize,
    incoming: bool,
    priority_pressure: Arc<AtomicBool>,
    inference_suspended: bool,
    dry_model_frame: Vec<f32>,
    mixed_model_frame: Vec<f32>,
    resampled_output: Vec<f32>,
    mapped_output: Vec<f32>,
    output_channels: usize,
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
        #[cfg(feature = "aec-prototype")]
        if config.aec.is_some()
            && (config.incoming
                || descriptor.channels.get() != 1
                || descriptor.sample_rate_hz.get() != 48000
                || descriptor.frame_samples_per_channel != 480
                || !(8000..=96000).contains(&config.input_format.sample_rate_hz.get()))
        {
            return Err(WorkerError::InvalidAecConfiguration);
        }
        if descriptor.channels.get() > 2
            || (config.incoming
                && (descriptor.channels != config.input_format.channels
                    || (descriptor.channels.get() == 2
                        && config.output_format.channels.get() != 2)))
        {
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
        let model_layout = ChannelLayout::from_channels(descriptor.channels);
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
        #[cfg(feature = "aec-prototype")]
        let fixed_latency = fixed_latency.saturating_add(if config.aec.is_some() {
            frames_duration(crate::AEC_DELAY_SAMPLES, 48000)
        } else {
            Duration::ZERO
        });
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
            model_channels: descriptor.channels.as_usize(),
            input_mapper,
            input_resampler,
            frame_assembler: ModelFrameAssembler::new(
                descriptor.channels,
                descriptor.frame_samples_per_channel,
            )?,
            input_meter: LevelMeter::new(0.85)?,
            input_scratch: vec![0.0; input_scratch_samples],
            mapped_input: vec![0.0; mapped_input_samples],
            resampled_input: vec![
                0.0;
                input_resampler_output_frames_max * descriptor.channels.as_usize()
            ],
            model: ModelStage {
                #[cfg(feature = "aec-prototype")]
                aec: None,
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
                output_meter: LevelMeter::new(0.85)?,
                output_resampler,
                output_mapper,
                wet_model_frame: vec![0.0; model_samples],
                planar_input: vec![0.0; model_samples],
                planar_output: vec![0.0; model_samples],
                model_channels: descriptor.channels.as_usize(),
                incoming: config.incoming,
                priority_pressure: config.priority_pressure.clone(),
                inference_suspended: false,
                dry_model_frame: vec![0.0; model_samples],
                mixed_model_frame: vec![0.0; model_samples],
                resampled_output: vec![
                    0.0;
                    output_resampler_output_frames_max
                        * descriptor.channels.as_usize()
                ],
                mapped_output: vec![0.0; mapped_output_samples],
                output_channels: config.output_format.channels.as_usize(),
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
            },
        })
    }

    fn run<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        mut input: WorkerInput,
        mut output: AudioRingProducer,
        commands: &Receiver<WorkerCommand>,
        cancelled: &AtomicBool,
        metrics: &RouteMetricsStore,
    ) -> Result<(), WorkerError> {
        let mut input_discontinuities = metrics.input_discontinuities();
        let mut output_discontinuities = metrics.output_discontinuities();
        #[cfg(feature = "aec-prototype")]
        let mut timed_fresh = true;
        while !cancelled.load(Ordering::Acquire) {
            self.apply_commands(reducer, commands, cancelled, metrics);
            if cancelled.load(Ordering::Acquire) {
                break;
            }

            match &mut input {
                WorkerInput::Samples(input) => {
                    input.trim_oldest_to(self.input_stale_target_frames);
                }
                #[cfg(feature = "aec-prototype")]
                WorkerInput::Timed(input) => {
                    if input.control().queued_frames() > self.input_stale_target_frames {
                        input.control().discontinue();
                    }
                }
            }
            let next_input_discontinuities = metrics.input_discontinuities();
            let next_output_discontinuities = metrics.output_discontinuities();
            if next_input_discontinuities != input_discontinuities
                || next_output_discontinuities != output_discontinuities
            {
                self.reset_after_discontinuity(reducer, metrics);
                #[cfg(feature = "aec-prototype")]
                {
                    timed_fresh = true;
                }
                // Audio captured during reset is stale too. Discard it once and acknowledge the
                // resulting discontinuity as part of this recovery, rather than resetting again.
                match &mut input {
                    WorkerInput::Samples(input) => {
                        input.trim_oldest_to(0);
                    }
                    #[cfg(feature = "aec-prototype")]
                    WorkerInput::Timed(input) => {
                        input.control().discontinue();
                    }
                }
                input_discontinuities = metrics.input_discontinuities();
                output_discontinuities = metrics.output_discontinuities();
            }
            #[cfg(feature = "aec-prototype")]
            if let WorkerInput::Timed(reader) = &mut input {
                let Some(block) = reader.read() else {
                    thread::park_timeout(IDLE_PARK);
                    continue;
                };
                if !reader.is_current(&block) {
                    continue;
                }
                if block.discontinuity && !timed_fresh {
                    self.reset_after_discontinuity(reducer, metrics);
                    // Native reset may be slow. Revoke this block and everything captured
                    // during reset, then accept the next fresh block without resetting twice.
                    reader.control().discontinue();
                    input_discontinuities = metrics.input_discontinuities();
                    output_discontinuities = metrics.output_discontinuities();
                    timed_fresh = true;
                    // Drain revoked blocks now so their old occupancy cannot trigger another
                    // stale-input reset at the top of the loop. At most 64 pops; a concurrently
                    // arriving first fresh block may also be discarded as part of this reset.
                    let _ = reader.read();
                    continue;
                }
                timed_fresh = false;
                if let Some(aec) = &mut self.model.aec {
                    aec.observe_capture(&block);
                }
                for samples in block.samples().chunks(self.input_scratch.len()) {
                    if !reader.is_current(&block) {
                        break;
                    }
                    self.input_scratch[..samples.len()].copy_from_slice(samples);
                    self.process_input(reducer, &mut output, samples.len(), metrics)?;
                }
                continue;
            }
            let input = match &mut input {
                WorkerInput::Samples(input) => input,
                #[cfg(feature = "aec-prototype")]
                WorkerInput::Timed(_) => unreachable!("timed input processed above"),
            };
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
                    self.model.denoise_enabled = settings.enabled;
                    if reducer.update_settings(&settings).is_err() {
                        metrics.record_model_error();
                        self.model.bypassed = true;
                    }
                }
                Ok(WorkerCommand::SetBypass(bypassed)) => self.model.bypassed = bypassed,
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
            .process(&self.mapped_input[..mapped_frames * self.model_channels]);
        metrics.record_input_level(input_level.peak, input_level.rms);

        let mut input_offset = 0;
        while input_offset < mapped_frames || self.input_resampler.pending_output_frames() > 0 {
            let progress = self.input_resampler.process(
                &self.mapped_input
                    [input_offset * self.model_channels..mapped_frames * self.model_channels],
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
        let samples = &self.resampled_input[..frames * self.model_channels];
        let model = &mut self.model;
        self.frame_assembler.push(samples, |model_input| {
            if frame_error.is_none() {
                frame_error = model
                    .process_frame(reducer, output, metrics, model_input)
                    .err();
            }
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
        #[cfg(feature = "aec-prototype")]
        if let Some(aec) = &mut self.model.aec {
            aec.reset_capture();
        }
        self.frame_assembler.reset();
        self.model.dry_delay.reset();
        self.model.output_resampler.reset();
        self.model.drift_controller.reset();
        metrics.record_drift_correction(0);
        self.model
            .crossfade
            .set_target(audio_dsp::CrossfadeTarget::Dry);
        if self.model.incoming
            && (self.model.bypassed
                || !self.model.denoise_enabled
                || self.model.priority_pressure.load(Ordering::Acquire))
        {
            self.model.inference_suspended = true;
            return;
        }
        if reset_reducer(reducer, metrics) {
            self.model.inference_suspended = false;
        } else {
            self.model.overload_guard.enter_degraded();
            metrics.set_degraded(true);
        }
    }
}

fn reset_reducer<R: NoiseReducer>(reducer: &mut R, metrics: &RouteMetricsStore) -> bool {
    let started = Instant::now();
    let result = reducer.reset();
    metrics.record_reset_duration(started.elapsed());
    if result.is_ok() {
        metrics.record_model_reset();
        true
    } else {
        metrics.record_model_error();
        false
    }
}

impl ModelStage {
    fn process_frame<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        output: &mut AudioRingProducer,
        metrics: &RouteMetricsStore,
        model_input: &[f32],
    ) -> Result<(), WorkerError> {
        #[cfg(feature = "aec-prototype")]
        let mut aec_output = [0.0; aec_webrtc::FRAME_SAMPLES];
        #[cfg(feature = "aec-prototype")]
        let (model_input, aec_elapsed) = if let Some(aec) = &mut self.aec {
            let elapsed = aec.process(
                model_input,
                &mut aec_output,
                !self.overload_guard.is_degraded(),
            );
            (&aec_output[..], elapsed)
        } else {
            (model_input, Duration::ZERO)
        };
        #[cfg(not(feature = "aec-prototype"))]
        let aec_elapsed = Duration::ZERO;
        let Self {
            #[cfg(feature = "aec-prototype")]
                aec: _,
            dry_delay,
            crossfade,
            limiter,
            output_meter,
            output_resampler,
            output_mapper,
            wet_model_frame,
            planar_input,
            planar_output,
            model_channels,
            incoming,
            priority_pressure,
            inference_suspended,
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
        } = self;
        let (output_channels, bypassed, denoise_enabled, model_deadline) = (
            *output_channels,
            *bypassed,
            *denoise_enabled,
            *model_deadline,
        );
        dry_delay.process(model_input, dry_model_frame)?;
        let priority_yield = *incoming && priority_pressure.load(Ordering::Acquire);
        let suspended = *incoming && (bypassed || !denoise_enabled || priority_yield);
        metrics.set_degraded(priority_yield || overload_guard.is_degraded());
        if suspended {
            *inference_suspended = true;
            metrics.record_skipped_inference();
            wet_model_frame.copy_from_slice(dry_model_frame);
            crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
        } else if overload_guard.should_process_model() {
            // Only the first frame of a contiguous recovery trial needs a reset.
            // Reset cost is observable separately; subsequent inference deadlines measure live work.
            let needs_reset = overload_guard.take_reset_request() | *inference_suspended;
            *inference_suspended = false;
            let probe_reset_failed = needs_reset && !reset_reducer(reducer, metrics);
            if probe_reset_failed {
                // Failed clean-state restoration must not run inference or trigger a second
                // reset in the same frame. Stay dry until the next bounded recovery trial.
                metrics.record_skipped_inference();
                overload_guard.enter_degraded();
                metrics.set_degraded(true);
                wet_model_frame.copy_from_slice(dry_model_frame);
                crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
            } else {
                let started = Instant::now();
                let frames = model_input.len() / *model_channels;
                for (frame, samples) in model_input.chunks_exact(*model_channels).enumerate() {
                    for (channel, sample) in samples.iter().enumerate() {
                        planar_input[channel * frames + frame] = *sample;
                    }
                }
                let model_result = reducer.process_planar(planar_input, planar_output);
                for (frame, samples) in wet_model_frame
                    .chunks_exact_mut(*model_channels)
                    .enumerate()
                {
                    for (channel, sample) in samples.iter_mut().enumerate() {
                        *sample = planar_output[channel * frames + frame];
                    }
                }
                let elapsed = started.elapsed().saturating_add(aec_elapsed);
                let model_failed = model_result.is_err();
                let (p95_us, p99_us) = inference_history.record(elapsed);
                metrics.record_inference(elapsed, model_deadline, p95_us, p99_us);
                match overload_guard.record_attempt(model_failed || elapsed > model_deadline) {
                    OverloadTransition::EnteredDegraded => metrics.set_degraded(true),
                    OverloadTransition::ExitedDegraded => metrics.set_degraded(false),
                    OverloadTransition::None => {}
                }

                if model_failed {
                    metrics.record_model_error();
                    wet_model_frame.copy_from_slice(dry_model_frame);
                    crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
                    if !reset_reducer(reducer, metrics) {
                        overload_guard.enter_degraded();
                        metrics.set_degraded(true);
                    }
                } else if bypassed || !denoise_enabled || overload_guard.is_degraded() {
                    crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
                } else {
                    crossfade.set_target(audio_dsp::CrossfadeTarget::Wet);
                }
            }
        } else {
            metrics.record_skipped_inference();
            wet_model_frame.copy_from_slice(dry_model_frame);
            crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
        }

        crossfade.process(dry_model_frame, wet_model_frame, mixed_model_frame)?;
        limiter.process_in_place(mixed_model_frame);

        let mut input_offset = 0;
        while input_offset < mixed_model_frame.len() || output_resampler.pending_output_frames() > 0
        {
            let progress =
                output_resampler.process(&mixed_model_frame[input_offset..], resampled_output)?;
            if progress.input_frames_consumed == 0 && progress.output_frames_written == 0 {
                return Err(WorkerError::ResamplerStalled);
            }
            input_offset += progress.input_frames_consumed * *model_channels;
            if progress.output_frames_written > 0 {
                let rendered =
                    &mut resampled_output[..progress.output_frames_written * *model_channels];
                limiter.process_in_place(rendered);
                let level = output_meter.process(rendered);
                metrics.record_output_level(level.peak, level.rms);
                let mapped_samples = progress.output_frames_written * output_channels;
                output_mapper.process(
                    &resampled_output[..progress.output_frames_written * *model_channels],
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

    #[cfg(feature = "aec-prototype")]
    #[test]
    fn timestamped_worker_accepts_initial_capture_and_recovers_once_per_discontinuity() {
        let (mut capture, reader) = audio_cpal::audio_tap_with_capacity(
            format(1),
            audio_cpal::StreamDirection::Input,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
            4800,
        )
        .unwrap();
        let (_dummy, input) = bounded_audio_ring(4800, format(1).channels).unwrap();
        let (output, mut sink) = bounded_audio_ring(9600, format(1).channels).unwrap();
        let metrics =
            RouteMetricsStore::new(input.metrics(), output.metrics(), format(1), format(1))
                .with_timed_input(reader.control());
        let mut config = PipelineWorkerConfig::new(format(1), format(1), 3840);
        config.aec = Some(crate::AecDiagnostics::default());
        let now = audio_cpal::MonotonicClock::default().now_ns();
        capture
            .submit_synthetic(&[0.1; 960], 1_010_000_000, 1_000_000_000, now)
            .unwrap();
        let mut worker = spawn_pipeline_worker_controlled(
            config,
            WorkerInput::Timed(reader),
            output,
            metrics.clone(),
            || Ok(fake()),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(wait_for_output(&sink, 480));
        assert_eq!(
            metrics.snapshot().model_resets,
            0,
            "fresh startup must not reset a warmed model"
        );
        let mut drained = [0.0; 960];
        sink.read(&mut drained).unwrap();
        capture.control().discontinue();
        worker.wake();
        let deadline = Instant::now() + Duration::from_secs(5);
        while metrics.snapshot().model_resets == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(metrics.snapshot().model_resets, 1);
        capture
            .submit_synthetic(&[0.2; 960], 1_030_000_000, 1_020_000_000, now + 20_000_000)
            .unwrap();
        worker.wake();
        assert!(wait_for_output(&sink, 480));
        worker.shutdown().unwrap();
        assert_eq!(
            metrics.snapshot().model_resets,
            1,
            "first post-reset block must not trigger a second reset"
        );
        assert_eq!(metrics.model_errors(), 0);
        assert!(
            capture.control().is_active(),
            "worker Stop does not own the callback generation"
        );
    }

    #[cfg(feature = "aec-prototype")]
    #[test]
    fn aec_output_is_exactly_the_denoiser_input_even_in_denoise_bypass() {
        struct Recorder {
            inner: FakeNoiseReducer,
            seen: [f32; 480],
        }
        impl NoiseReducer for Recorder {
            fn descriptor(&self) -> &NoiseReducerDescriptor {
                self.inner.descriptor()
            }
            fn process_planar(
                &mut self,
                input: &[f32],
                output: &mut [f32],
            ) -> Result<NoiseReductionStats, DenoiseError> {
                self.seen.copy_from_slice(input);
                self.inner.process_planar(input, output)
            }
            fn reset(&mut self) -> Result<(), DenoiseError> {
                self.inner.reset()
            }
            fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
                self.inner.update_settings(settings)
            }
        }
        for bypass in [false, true] {
            let (mut expected_aec, _c1, _r1, _return1) = crate::aec::tests::stage();
            let (actual_aec, _c2, _r2, _return2) = crate::aec::tests::stage();
            let (_tx, input) = bounded_audio_ring(4800, format(1).channels).unwrap();
            let (mut output, mut sink) = bounded_audio_ring(9600, format(1).channels).unwrap();
            let metrics =
                RouteMetricsStore::new(input.metrics(), output.metrics(), format(1), format(1));
            let mut config = PipelineWorkerConfig::new(format(1), format(1), 3840);
            config.initially_bypassed = bypass;
            config.aec = Some(crate::AecDiagnostics::default());
            let mut reducer = Recorder {
                inner: fake_with(480, FakeFailureMode::Never),
                seen: [0.0; 480],
            };
            let mut graph =
                PipelineGraph::new(&config, reducer.descriptor(), &metrics, 9600).unwrap();
            graph.model.aec = Some(actual_aec);
            let mut expected = [0.0; 480];
            let mut dry = FixedDryDelay::new(format(1).channels, 480).unwrap();
            let mut expected_dry = [0.0; 480];
            for frame in 0..60 {
                crate::aec::tests::tick(&mut expected_aec, frame);
                crate::aec::tests::tick(graph.model.aec.as_mut().unwrap(), frame);
                let mut capture = [0.0; 480];
                for (i, x) in capture.iter_mut().enumerate() {
                    *x = 0.1 * ((frame * 480 + i as u64) as f32 * 0.03).sin();
                }
                expected_aec.process(&capture, &mut expected, true);
                dry.process(&expected, &mut expected_dry).unwrap();
                graph
                    .model
                    .process_frame(&mut reducer, &mut output, &metrics, &capture)
                    .unwrap();
                assert_eq!(reducer.seen, expected, "bypass={bypass}, frame={frame}");
                let mut rendered = [0.0; 480];
                sink.read(&mut rendered).unwrap();
                // The adjustable output resampler has its own delay; inspect the denoiser's dry
                // branch before output conversion to verify its exact additional DFN delay.
                assert_eq!(graph.model.dry_model_frame, expected_dry);
            }
            assert_eq!(metrics.model_errors(), 0);
        }
    }

    #[cfg(feature = "aec-prototype")]
    #[test]
    #[ignore = "wall-clock AEC + production DFN benchmark: run explicitly with --release"]
    #[allow(clippy::assertions_on_constants)]
    fn production_aec_dfn_graph_timing() {
        assert!(!cfg!(debug_assertions), "run with --release");
        let (aec, _capture, _render, _returned) = crate::aec::tests::stage();
        let (_tx, input) = bounded_audio_ring(4800, format(1).channels).unwrap();
        let (mut output, mut sink) = bounded_audio_ring(9600, format(1).channels).unwrap();
        let metrics =
            RouteMetricsStore::new(input.metrics(), output.metrics(), format(1), format(1));
        let mut config = PipelineWorkerConfig::new(format(1), format(1), 3840);
        config.aec = Some(crate::AecDiagnostics::default());
        let mut reducer = denoise_dfn::DfnNoiseReducer::load(
            denoise_dfn::default_model_path(),
            DenoiseSettings::default(),
        )
        .unwrap();
        reducer.warm_up(3).unwrap();
        let mut graph = PipelineGraph::new(&config, reducer.descriptor(), &metrics, 9600).unwrap();
        graph.model.aec = Some(aec);
        let mut timing = Vec::with_capacity(1000);
        let burner_stop = Arc::new(AtomicBool::new(false));
        // Optional hardware-free scheduling diagnostic: wake the graph at a 10 ms
        // cadence instead of running continuously on a hot CPU. Synthetic timestamps
        // and device-free output still do not measure hardware scheduling or latency.
        let paced = std::env::var("OPENKRISP_AEC_PACED").is_ok_and(|value| value == "1");
        let burners = std::env::var("OPENKRISP_AEC_BURNERS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(4);
        let mut burner_threads = Vec::new();
        for _ in 0..burners {
            let stop = burner_stop.clone();
            burner_threads.push(thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(if paced { 25 } else { 10 });
                let mut x = 0.1_f64;
                while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
                    for _ in 0..10000 {
                        x = std::hint::black_box(x.sin() + 0.1);
                    }
                }
            }));
        }
        let mut discarded = [0.0; 960];
        let mut misses = 0;
        let mut next_frame = Instant::now();
        for frame in 0..1100 {
            if paced {
                thread::sleep(next_frame.saturating_duration_since(Instant::now()));
                next_frame = Instant::now() + Duration::from_millis(10);
            }
            crate::aec::tests::tick(graph.model.aec.as_mut().unwrap(), frame);
            for (i, x) in graph.input_scratch.iter_mut().enumerate() {
                *x = 0.1 * ((frame * 480 + i as u64) as f32 * 0.03).sin();
            }
            let start = Instant::now();
            graph
                .process_input(&mut reducer, &mut output, 480, &metrics)
                .unwrap();
            let elapsed = start.elapsed().as_micros() as u64;
            if frame >= 100 {
                timing.push(elapsed);
                misses += usize::from(elapsed > 10000);
            }
            sink.read(&mut discarded).unwrap();
        }
        timing.sort_unstable();
        burner_stop.store(true, Ordering::Release);
        for thread in burner_threads {
            thread.join().unwrap();
        }
        let aec = graph.model.aec.as_ref().unwrap().snapshot();
        eprintln!("AEC diagnostics={aec:?}, CPU burners={burners}, paced={paced}");
        eprintln!(
            "AEC+DFN full graph frames=1000 p50={}us p95={}us p99={}us max={}us missed={} metrics={:?}",
            percentile(&timing, 50),
            percentile(&timing, 95),
            percentile(&timing, 99),
            timing[999],
            misses,
            metrics.snapshot()
        );
        assert_eq!(metrics.model_errors(), 0);
        assert!(
            aec.processed_frames >= 1000,
            "benchmark must exercise actual AEC, not just fallback"
        );
    }

    #[test]
    fn cancellation_before_startup_does_not_construct_a_model() {
        let channels = ChannelCount::new(1).expect("mono");
        let (_producer, input) = bounded_audio_ring(4_800, channels).expect("input");
        let (output, _sink) = bounded_audio_ring(4_800, channels).expect("output");
        let metrics =
            RouteMetricsStore::new(input.metrics(), output.metrics(), format(1), format(1));
        let result = spawn_pipeline_worker_controlled::<FakeNoiseReducer, _>(
            PipelineWorkerConfig::new(format(1), format(1), 3_840),
            input.into(),
            output,
            metrics,
            || panic!("a cancelled startup must not invoke the factory"),
            &AtomicBool::new(true),
        );
        assert!(matches!(result, Err(WorkerStartError::Cancelled)));
    }

    #[test]
    fn failed_recovery_reset_skips_inference_and_waits_for_the_next_trial() {
        struct ResetFailure {
            inner: FakeNoiseReducer,
            resets: usize,
        }
        impl NoiseReducer for ResetFailure {
            fn descriptor(&self) -> &NoiseReducerDescriptor {
                self.inner.descriptor()
            }
            fn process_planar(
                &mut self,
                _: &[f32],
                _: &mut [f32],
            ) -> Result<NoiseReductionStats, DenoiseError> {
                panic!("no inference after a failed clean-state reset");
            }
            fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
                self.inner.update_settings(settings)
            }
            fn reset(&mut self) -> Result<(), DenoiseError> {
                self.resets += 1;
                Err(DenoiseError::InferenceFailed(
                    "injected reset failure".to_owned(),
                ))
            }
        }
        let channels = ChannelCount::new(1).expect("mono");
        let (_producer, input) = bounded_audio_ring(4_800, channels).expect("input");
        let (mut output, mut sink) = bounded_audio_ring(4_800, channels).expect("output");
        let metrics =
            RouteMetricsStore::new(input.metrics(), output.metrics(), format(1), format(1));
        let mut reducer = ResetFailure {
            inner: fake(),
            resets: 0,
        };
        let mut graph = PipelineGraph::new(
            &PipelineWorkerConfig::new(format(1), format(1), 3_840),
            reducer.descriptor(),
            &metrics,
            output.capacity_frames(),
        )
        .expect("graph");
        graph.model.overload_guard.enter_degraded();
        let mut rendered = [0.0; 480];
        for _ in 0..150 {
            graph
                .model
                .process_frame(&mut reducer, &mut output, &metrics, &[0.1; 480])
                .expect("dry fallback");
            sink.read_or_silence(&mut rendered).expect("sink");
        }
        assert_eq!(reducer.resets, 1);
        assert_eq!(metrics.model_errors(), 1);
        assert_eq!(metrics.snapshot().skipped_inferences, 150);
        assert!(metrics.snapshot().degraded);
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
        assert_eq!(snapshot.discontinuities, 2); // overflow plus the recovery discard
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
        assert_eq!(snapshot.model_resets, 0); // recovery cooldown has not elapsed
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

    #[test]
    fn final_output_is_limited_and_metered_after_resampling() {
        let mono = ChannelCount::new(1).expect("mono");
        for rate in [44_100, 48_000, 96_000] {
            for correction in [-1_000, 0, 1_000] {
                let (_, input) = bounded_audio_ring(4_800, mono).expect("input");
                let (mut output, mut sink) = bounded_audio_ring(19_200, mono).expect("output");
                let metrics = RouteMetricsStore::new(
                    input.metrics(),
                    output.metrics(),
                    format(1),
                    format_at(rate, 1),
                );
                let mut config = PipelineWorkerConfig::new(format(1), format_at(rate, 1), 4_800);
                config.initially_bypassed = true;
                let mut reducer = fake();
                let mut graph = PipelineGraph::new(&config, reducer.descriptor(), &metrics, 19_200)
                    .expect("graph");
                let frame: Vec<_> = (0..480)
                    .map(|i| if (i / 80) % 2 == 0 { 0.98 } else { -0.98 })
                    .collect();
                let mut rendered = vec![0.0; 19_200];
                let mut peak = 0.0_f32;
                for _ in 0..16 {
                    graph
                        .model
                        .output_resampler
                        .set_correction_ppm(correction)
                        .expect("ratio");
                    graph
                        .model
                        .process_frame(&mut reducer, &mut output, &metrics, &frame)
                        .expect("frame");
                    let read = sink.read(&mut rendered).expect("read");
                    for sample in &rendered[..read.read_frames] {
                        assert!(
                            sample.is_finite() && sample.abs() <= 0.98,
                            "rate={rate}, correction={correction}, sample={sample}"
                        );
                        peak = peak.max(sample.abs());
                    }
                }
                assert!((peak - 0.98).abs() < 1.0e-6);
                assert!((metrics.snapshot().output_peak - peak).abs() < 1.0e-6);
            }
        }
    }

    #[test]
    fn audio_accumulated_during_reset_does_not_trigger_another_reset() {
        struct ResetGate {
            inner: FakeNoiseReducer,
            entered: SyncSender<()>,
            release: Receiver<()>,
        }
        impl NoiseReducer for ResetGate {
            fn descriptor(&self) -> &NoiseReducerDescriptor {
                self.inner.descriptor()
            }
            fn process_planar(
                &mut self,
                input: &[f32],
                output: &mut [f32],
            ) -> Result<NoiseReductionStats, DenoiseError> {
                self.inner.process_planar(input, output)
            }
            fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
                self.inner.update_settings(settings)
            }
            fn reset(&mut self) -> Result<(), DenoiseError> {
                self.entered.try_send(()).expect("one reset");
                self.release
                    .recv_timeout(Duration::from_secs(2))
                    .expect("release reset");
                self.inner.reset()
            }
        }
        let mono = ChannelCount::new(1).expect("mono");
        let (mut source, input) = bounded_audio_ring(960, mono).expect("input");
        let (output, sink) = bounded_audio_ring(4_800, mono).expect("output");
        let metrics =
            RouteMetricsStore::new(input.metrics(), output.metrics(), format(1), format(1));
        let (entered, entered_rx) = sync_channel(1);
        let (release, release_rx) = sync_channel(1);
        let mut worker = spawn_pipeline_worker(
            PipelineWorkerConfig::new(format(1), format(1), 480),
            input,
            output,
            metrics.clone(),
            move || {
                Ok(ResetGate {
                    inner: fake(),
                    entered,
                    release: release_rx,
                })
            },
        )
        .expect("worker");
        source.write(&[0.2; 960]).expect("trigger stale trim");
        worker.wake();
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("reset started");
        source
            .write(&[0.3; 1_440])
            .expect("capture continues during reset");
        release.send(()).expect("reset finished");
        let deadline = Instant::now() + Duration::from_secs(2);
        while source.metrics().fill_frames() != 0 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        source.write(&[0.4; 480]).expect("fresh input");
        worker.wake();
        assert!(wait_for_drained_input(&source, &sink));
        worker.shutdown().expect("join");
        assert_eq!(metrics.snapshot().model_resets, 1);
        assert!(entered_rx.try_recv().is_err());
    }
}
