//! The production processing core shared by the threaded runner and deterministic backend.
use super::*;

#[derive(Clone, Copy, Default)]
pub(crate) enum ProcessingClock {
    #[default]
    Wall,
    Simulated(Duration),
}
impl ProcessingClock {
    fn start(self) -> Option<Instant> {
        match self {
            Self::Wall => Some(Instant::now()),
            Self::Simulated(_) => None,
        }
    }
    fn elapsed(self, started: Option<Instant>) -> Duration {
        match self {
            Self::Wall => started.map_or(Duration::ZERO, |time| time.elapsed()),
            Self::Simulated(cost) => cost,
        }
    }
}

#[derive(Default)]
struct Progress {
    input_discontinuities: u64,
    output_discontinuities: u64,
    #[cfg(feature = "aec-prototype")]
    timed_fresh: bool,
}
pub(crate) struct PipelineGraph {
    progress: Progress,
    pub(super) input_stale_target_frames: usize,
    pub(super) input_channels: usize,
    pub(super) model_channels: usize,
    pub(super) input_mapper: ChannelMapper,
    pub(super) input_resampler: StreamingResampler,
    pub(super) frame_assembler: ModelFrameAssembler,
    pub(super) input_meter: LevelMeter,
    pub(super) input_scratch: Vec<f32>,
    pub(super) mapped_input: Vec<f32>,
    pub(super) resampled_input: Vec<f32>,
    pub(super) model: ModelStage,
}

/// Model-rate processing and the final device-rate output stage own their reusable storage.
pub(super) struct ModelStage {
    pub(super) clock: ProcessingClock,
    #[cfg(feature = "aec-prototype")]
    pub(super) aec: Option<crate::aec::AecStage>,
    pub(super) dry_delay: FixedDryDelay,
    pub(super) crossfade: EqualPowerCrossfade,
    pub(super) limiter: SafetyLimiter,
    pub(super) output_meter: LevelMeter,
    pub(super) output_resampler: StreamingResampler,
    pub(super) output_mapper: ChannelMapper,
    pub(super) wet_model_frame: Vec<f32>,
    pub(super) planar_input: Vec<f32>,
    pub(super) planar_output: Vec<f32>,
    pub(super) model_channels: usize,
    pub(super) incoming: bool,
    pub(super) priority_pressure: Arc<AtomicBool>,
    pub(super) inference_suspended: bool,
    pub(super) dry_model_frame: Vec<f32>,
    pub(super) mixed_model_frame: Vec<f32>,
    pub(super) resampled_output: Vec<f32>,
    pub(super) mapped_output: Vec<f32>,
    pub(super) output_channels: usize,
    pub(super) bypassed: bool,
    pub(super) denoise_enabled: bool,
    pub(super) model_deadline: Duration,
    inference_history: InferenceHistory,
    pub(super) drift_controller: DriftController,
    pub(super) overload_guard: OverloadGuard,
}

impl PipelineGraph {
    pub(crate) fn new(
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
            progress: Progress {
                #[cfg(feature = "aec-prototype")]
                timed_fresh: true,
                ..Progress::default()
            },
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
                clock: ProcessingClock::Wall,
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

    /// Runs one bounded worker iteration, with the same discontinuity policy as production.
    pub(crate) fn step<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        input: &mut WorkerInput,
        output: &mut AudioRingProducer,
        metrics: &RouteMetricsStore,
    ) -> Result<bool, WorkerError> {
        match input {
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
        if next_input_discontinuities != self.progress.input_discontinuities
            || next_output_discontinuities != self.progress.output_discontinuities
        {
            self.reset_after_discontinuity(reducer, metrics);
            #[cfg(feature = "aec-prototype")]
            {
                self.progress.timed_fresh = true;
            }
            // Audio captured during reset is stale too. Discard it once and acknowledge the
            // resulting discontinuity as part of this recovery, rather than resetting again.
            match input {
                WorkerInput::Samples(input) => {
                    input.trim_oldest_to(0);
                }
                #[cfg(feature = "aec-prototype")]
                WorkerInput::Timed(input) => {
                    input.control().discontinue();
                }
            }
            self.progress.input_discontinuities = metrics.input_discontinuities();
            self.progress.output_discontinuities = metrics.output_discontinuities();
        }
        #[cfg(feature = "aec-prototype")]
        if let WorkerInput::Timed(reader) = input {
            let Some(block) = reader.read() else {
                return Ok(false);
            };
            if !reader.is_current(&block) {
                return Ok(true);
            }
            if block.discontinuity && !self.progress.timed_fresh {
                self.reset_after_discontinuity(reducer, metrics);
                // Native reset may be slow. Revoke this block and everything captured
                // during reset, then accept the next fresh block without resetting twice.
                reader.control().discontinue();
                self.progress.input_discontinuities = metrics.input_discontinuities();
                self.progress.output_discontinuities = metrics.output_discontinuities();
                self.progress.timed_fresh = true;
                // Drain revoked blocks now so their old occupancy cannot trigger another
                // stale-input reset at the top of the loop. At most 64 pops; a concurrently
                // arriving first fresh block may also be discarded as part of this reset.
                let _ = reader.read();
                return Ok(true);
            }
            self.progress.timed_fresh = false;
            if let Some(aec) = &mut self.model.aec {
                aec.observe_capture(&block);
            }
            for samples in block.samples().chunks(self.input_scratch.len()) {
                if !reader.is_current(&block) {
                    break;
                }
                self.input_scratch[..samples.len()].copy_from_slice(samples);
                self.process_input(reducer, output, samples.len(), metrics)?;
            }
            return Ok(true);
        }
        #[cfg(feature = "aec-prototype")]
        let input = match input {
            WorkerInput::Samples(input) => input,
            WorkerInput::Timed(_) => unreachable!("timed input processed above"),
        };
        #[cfg(not(feature = "aec-prototype"))]
        let WorkerInput::Samples(input) = input;
        let read = input.read(&mut self.input_scratch)?;
        if read.read_frames == 0 {
            return Ok(false);
        }
        let input_samples = read.read_frames * self.input_channels;
        self.process_input(reducer, output, input_samples, metrics)?;
        Ok(true)
    }

    pub(crate) fn set_processing_cost(&mut self, cost: Duration) {
        self.model.clock = ProcessingClock::Simulated(cost);
    }

    pub(crate) fn reset<R: NoiseReducer>(&mut self, reducer: &mut R, metrics: &RouteMetricsStore) {
        self.reset_after_discontinuity(reducer, metrics);
    }

    pub(super) fn run<R: NoiseReducer>(
        &mut self,
        reducer: &mut R,
        mut input: WorkerInput,
        mut output: AudioRingProducer,
        commands: &Receiver<WorkerCommand>,
        cancelled: &AtomicBool,
        metrics: &RouteMetricsStore,
    ) -> Result<(), WorkerError> {
        self.progress.input_discontinuities = metrics.input_discontinuities();
        self.progress.output_discontinuities = metrics.output_discontinuities();
        while !cancelled.load(Ordering::Acquire) {
            self.apply_commands(reducer, commands, cancelled, metrics);
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if !self.step(reducer, &mut input, &mut output, metrics)? {
                thread::park_timeout(IDLE_PARK);
            }
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

    pub(super) fn process_input<R: NoiseReducer>(
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

    pub(super) fn reset_after_discontinuity<R: NoiseReducer>(
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
        if reset_reducer(reducer, metrics, self.model.clock) {
            self.model.inference_suspended = false;
        } else {
            self.model.overload_guard.enter_degraded();
            metrics.set_degraded(true);
        }
    }
}

fn reset_reducer<R: NoiseReducer>(
    reducer: &mut R,
    metrics: &RouteMetricsStore,
    clock: ProcessingClock,
) -> bool {
    let started = clock.start();
    let result = reducer.reset();
    metrics.record_reset_duration(clock.elapsed(started));
    if result.is_ok() {
        metrics.record_model_reset();
        true
    } else {
        metrics.record_model_error();
        false
    }
}

impl ModelStage {
    pub(super) fn process_frame<R: NoiseReducer>(
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
            clock,
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
            let probe_reset_failed = needs_reset && !reset_reducer(reducer, metrics, *clock);
            if probe_reset_failed {
                // Failed clean-state restoration must not run inference or trigger a second
                // reset in the same frame. Stay dry until the next bounded recovery trial.
                metrics.record_skipped_inference();
                overload_guard.enter_degraded();
                metrics.set_degraded(true);
                wet_model_frame.copy_from_slice(dry_model_frame);
                crossfade.set_target(audio_dsp::CrossfadeTarget::Dry);
            } else {
                let started = clock.start();
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
                let elapsed = match clock {
                    ProcessingClock::Wall => clock.elapsed(started).saturating_add(aec_elapsed),
                    // Injected cost describes the entire AEC + denoiser attempt. Real native
                    // AEC work still executes, but host scheduling cannot change this clock.
                    ProcessingClock::Simulated(_) => clock.elapsed(started),
                };
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
                    if !reset_reducer(reducer, metrics, *clock) {
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
    pub(super) samples_us: [u64; INFERENCE_WINDOW],
    pub(super) len: usize,
    pub(super) cursor: usize,
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

pub(super) fn percentile(ordered: &[u64], percentile: usize) -> u64 {
    if ordered.is_empty() {
        return 0;
    }
    let index = (ordered.len() - 1) * percentile / 100;
    ordered[index]
}

fn frames_duration(frames: usize, sample_rate_hz: u32) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(sample_rate_hz))
}
