use audio_protocol::{ChannelCount, PipelineState, SampleRateHz};
use denoise_core::{DenoiseError, NoiseReducer};

use crate::{
    AudioRingConsumer, AudioRingMetrics, AudioRingMetricsSnapshot, AudioRingProducer,
    AudioTransportError, CallbackEvent, FakeAudioSink, FakeAudioSource, FakeEndpointError,
    FakeEndpointStats, bounded_audio_ring,
};

/// Invalid configuration or lifecycle operation for a deterministic simulated route.
#[derive(Debug, thiserror::Error)]
pub enum SimulationError {
    #[error("simulation requires matching source, reducer, and sink sample rates")]
    SampleRateMismatch,
    #[error("simulation requires matching source, reducer, and sink channel counts")]
    ChannelCountMismatch,
    #[error("the Phase 1 harness currently supports mono reducer frames only")]
    OnlyMonoSupported,
    #[error("input target fill must not exceed input ring capacity")]
    InputTargetExceedsCapacity,
    #[error("output prefill must not exceed output ring capacity")]
    OutputPrefillExceedsCapacity,
    #[error("simulation duration overflowed its clock")]
    DurationOverflow,
    #[error("route cannot perform this operation while {0:?}")]
    InvalidState(PipelineState),
    #[error(transparent)]
    Endpoint(#[from] FakeEndpointError),
    #[error(transparent)]
    Transport(#[from] AudioTransportError),
    #[error(transparent)]
    Denoise(#[from] DenoiseError),
}

/// Deterministic route results and bounded queue health.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimulationReport {
    pub state: PipelineState,
    pub elapsed_ns: u64,
    pub source: FakeEndpointStats,
    pub sink: FakeEndpointStats,
    pub input_ring: AudioRingMetricsSnapshot,
    pub output_ring: AudioRingMetricsSnapshot,
    pub reducer_failures: u64,
}

/// Single-threaded event simulator for callback timing, bounded transport, and reducer fallback.
///
/// This is deliberately not the production worker from Phase 2. It schedules independent fake
/// endpoint clocks in timestamp order and uses the same non-blocking ring APIs that callbacks will
/// use later.
pub struct SimulatedRoute<R: NoiseReducer> {
    state: PipelineState,
    source: FakeAudioSource,
    sink: FakeAudioSink,
    reducer: R,
    input_producer: AudioRingProducer,
    input_consumer: AudioRingConsumer,
    output_producer: AudioRingProducer,
    output_consumer: AudioRingConsumer,
    input_metrics: AudioRingMetrics,
    output_metrics: AudioRingMetrics,
    input_target_fill_frames: usize,
    model_frame_samples: usize,
    source_scratch: Vec<f32>,
    sink_scratch: Vec<f32>,
    model_input: Vec<f32>,
    model_output: Vec<f32>,
    elapsed_ns: u64,
    reducer_failures: u64,
}

impl<R: NoiseReducer> SimulatedRoute<R> {
    /// Allocates all route buffers and applies output startup prefill.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: FakeAudioSource,
        sink: FakeAudioSink,
        reducer: R,
        input_capacity_frames: usize,
        output_capacity_frames: usize,
        input_target_fill_frames: usize,
        output_prefill_frames: usize,
    ) -> Result<Self, SimulationError> {
        let (source_rate, source_channels) = source.format();
        let (sink_rate, sink_channels) = sink.format();
        let descriptor = reducer.descriptor();
        validate_formats(
            source_rate,
            source_channels,
            sink_rate,
            sink_channels,
            descriptor.sample_rate_hz,
            descriptor.channels,
        )?;
        if source_channels.get() != 1 {
            return Err(SimulationError::OnlyMonoSupported);
        }
        if input_target_fill_frames > input_capacity_frames {
            return Err(SimulationError::InputTargetExceedsCapacity);
        }
        if output_prefill_frames > output_capacity_frames {
            return Err(SimulationError::OutputPrefillExceedsCapacity);
        }
        let model_frame_samples = descriptor.frame_samples()?;
        let source_scratch_samples = source
            .max_callback_frames()
            .checked_mul(source_channels.as_usize())
            .ok_or(FakeEndpointError::BufferSizeOverflow)?;
        let sink_scratch_samples = sink
            .max_callback_frames()
            .checked_mul(sink_channels.as_usize())
            .ok_or(FakeEndpointError::BufferSizeOverflow)?;
        let (input_producer, input_consumer) =
            bounded_audio_ring(input_capacity_frames, source_channels)?;
        let (mut output_producer, output_consumer) =
            bounded_audio_ring(output_capacity_frames, sink_channels)?;
        let input_metrics = input_producer.metrics();
        let output_metrics = output_producer.metrics();
        if output_prefill_frames > 0 {
            let silence = vec![0.0; output_prefill_frames * sink_channels.as_usize()];
            let report = output_producer.write(&silence)?;
            debug_assert_eq!(report.written_frames, output_prefill_frames);
        }

        Ok(Self {
            state: PipelineState::Stopped,
            source,
            sink,
            reducer,
            input_producer,
            input_consumer,
            output_producer,
            output_consumer,
            input_metrics,
            output_metrics,
            input_target_fill_frames,
            model_frame_samples,
            source_scratch: vec![0.0; source_scratch_samples],
            sink_scratch: vec![0.0; sink_scratch_samples],
            model_input: vec![0.0; model_frame_samples],
            model_output: vec![0.0; model_frame_samples],
            elapsed_ns: 0,
            reducer_failures: 0,
        })
    }

    /// Starts callback scheduling. Duplicate starts are rejected.
    pub fn start(&mut self) -> Result<(), SimulationError> {
        if self.state != PipelineState::Stopped {
            return Err(SimulationError::InvalidState(self.state));
        }
        self.state = PipelineState::Starting;
        self.state = PipelineState::Running;
        Ok(())
    }

    /// Runs timestamp-ordered fake callbacks for an additional simulated duration.
    pub fn run_for_ns(&mut self, duration_ns: u64) -> Result<SimulationReport, SimulationError> {
        if self.state != PipelineState::Running {
            return Err(SimulationError::InvalidState(self.state));
        }
        let target_ns = self
            .elapsed_ns
            .checked_add(duration_ns)
            .ok_or(SimulationError::DurationOverflow)?;

        loop {
            let source_deadline = self.source.next_deadline_ns();
            let sink_deadline = self.sink.next_deadline_ns();
            let next_deadline = match (source_deadline, sink_deadline) {
                (Some(source), Some(sink)) => source.min(sink),
                (Some(source), None) => source,
                (None, Some(sink)) => sink,
                (None, None) => break,
            };
            if next_deadline > target_ns {
                break;
            }

            // Source wins equal timestamps so a same-clock sink can consume newly produced data.
            if source_deadline.is_some_and(|deadline| deadline <= sink_deadline.unwrap_or(u64::MAX))
            {
                self.run_source_callback()?;
            } else {
                self.run_sink_callback()?;
            }
            self.elapsed_ns = next_deadline;
            if self.state != PipelineState::Running {
                break;
            }
        }
        if self.state == PipelineState::Running {
            self.elapsed_ns = self.elapsed_ns.max(target_ns);
        }
        Ok(self.report())
    }

    /// Stops a running or failed simulation and resets the reducer state.
    pub fn stop(&mut self) -> Result<(), SimulationError> {
        if !matches!(self.state, PipelineState::Running | PipelineState::Failed) {
            return Err(SimulationError::InvalidState(self.state));
        }
        self.state = PipelineState::Stopping;
        self.reducer.reset()?;
        self.state = PipelineState::Stopped;
        Ok(())
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> PipelineState {
        self.state
    }

    /// Returns the sink for deterministic capture assertions.
    #[must_use]
    pub const fn sink(&self) -> &FakeAudioSink {
        &self.sink
    }

    /// Takes a current health report without advancing simulated time.
    #[must_use]
    pub fn report(&self) -> SimulationReport {
        SimulationReport {
            state: self.state,
            elapsed_ns: self.elapsed_ns,
            source: self.source.stats(),
            sink: self.sink.stats(),
            input_ring: self.input_metrics.snapshot(),
            output_ring: self.output_metrics.snapshot(),
            reducer_failures: self.reducer_failures,
        }
    }

    fn run_source_callback(&mut self) -> Result<(), SimulationError> {
        match self.source.next_callback(&mut self.source_scratch)? {
            CallbackEvent::Audio(callback) => {
                let samples = callback.frames * self.reducer.descriptor().channels.as_usize();
                self.input_producer.write(&self.source_scratch[..samples])?;
                self.pump_worker()?;
            }
            CallbackEvent::StreamError(_) => {}
            CallbackEvent::DeviceLost(_) => self.state = PipelineState::Failed,
        }
        Ok(())
    }

    fn run_sink_callback(&mut self) -> Result<(), SimulationError> {
        match self.sink.next_callback()? {
            CallbackEvent::Audio(callback) => {
                self.pump_worker()?;
                let samples = callback.frames * self.reducer.descriptor().channels.as_usize();
                self.output_consumer
                    .read_or_silence(&mut self.sink_scratch[..samples])?;
                self.sink.consume(&self.sink_scratch[..samples])?;
            }
            CallbackEvent::StreamError(_) => {}
            CallbackEvent::DeviceLost(_) => self.state = PipelineState::Failed,
        }
        Ok(())
    }

    fn pump_worker(&mut self) -> Result<(), SimulationError> {
        self.input_consumer
            .trim_oldest_to(self.input_target_fill_frames);
        loop {
            let input_fill = self.input_metrics.snapshot().fill_frames;
            let output = self.output_metrics.snapshot();
            let model_frames = self.model_frame_samples;
            if input_fill < model_frames
                || output.capacity_frames - output.fill_frames < model_frames
            {
                break;
            }
            let read = self.input_consumer.read(&mut self.model_input)?;
            debug_assert_eq!(read.read_frames, model_frames);
            if self
                .reducer
                .process_planar(&self.model_input, &mut self.model_output)
                .is_err()
            {
                self.reducer_failures += 1;
                self.model_output.copy_from_slice(&self.model_input);
            }
            let written = self.output_producer.write(&self.model_output)?;
            debug_assert_eq!(written.written_frames, model_frames);
        }
        Ok(())
    }
}

fn validate_formats(
    source_rate: SampleRateHz,
    source_channels: ChannelCount,
    sink_rate: SampleRateHz,
    sink_channels: ChannelCount,
    reducer_rate: SampleRateHz,
    reducer_channels: ChannelCount,
) -> Result<(), SimulationError> {
    if source_rate != sink_rate || source_rate != reducer_rate {
        return Err(SimulationError::SampleRateMismatch);
    }
    if source_channels != sink_channels || source_channels != reducer_channels {
        return Err(SimulationError::ChannelCountMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use audio_protocol::{DenoiseSettings, PipelineState};
    use denoise_core::{
        FakeFailureMode, FakeNoiseReducer, FakeNoiseReducerConfig, NoiseReducerDescriptor,
        NoiseReductionStats,
    };

    use super::*;
    use crate::{CallbackSchedule, FakeEndpointFault};

    fn rate() -> SampleRateHz {
        SampleRateHz::new(48_000).expect("valid rate")
    }

    fn mono() -> ChannelCount {
        ChannelCount::new(1).expect("valid channels")
    }

    fn fake_reducer(failure_mode: FakeFailureMode) -> FakeNoiseReducer {
        FakeNoiseReducer::new(FakeNoiseReducerConfig {
            descriptor: NoiseReducerDescriptor {
                model_id: "simulation-fake".to_owned(),
                sample_rate_hz: rate(),
                channels: mono(),
                frame_samples_per_channel: 480,
                algorithmic_latency_samples: 480,
            },
            settings: DenoiseSettings::default(),
            enabled_gain: 0.5,
            processing_delay: Duration::ZERO,
            failure_mode,
            stats: NoiseReductionStats::default(),
        })
        .expect("valid fake reducer")
    }

    fn route(
        source_drift_ppm: i32,
        source_faults: Vec<FakeEndpointFault>,
        failure_mode: FakeFailureMode,
    ) -> SimulatedRoute<FakeNoiseReducer> {
        let source = FakeAudioSource::new(
            rate(),
            mono(),
            source_drift_ppm,
            CallbackSchedule::new(vec![64, 96, 128, 240, 480, 512], vec![0, -500, 750])
                .expect("valid source schedule"),
            source_faults,
        )
        .expect("valid source");
        let sink = FakeAudioSink::new(
            rate(),
            mono(),
            0,
            CallbackSchedule::new(vec![96, 240, 480, 128], vec![250, 0, -250])
                .expect("valid sink schedule"),
            Vec::new(),
            48_000,
        )
        .expect("valid sink");
        SimulatedRoute::new(
            source,
            sink,
            fake_reducer(failure_mode),
            4_800,
            4_800,
            2_400,
            960,
        )
        .expect("valid simulated route")
    }

    #[test]
    fn lifecycle_random_callbacks_and_queues_remain_bounded() {
        let mut route = route(0, Vec::new(), FakeFailureMode::Never);
        assert_eq!(route.state(), PipelineState::Stopped);
        route.start().expect("route starts");
        assert!(matches!(
            route.start(),
            Err(SimulationError::InvalidState(PipelineState::Running))
        ));
        let report = route
            .run_for_ns(2_000_000_000)
            .expect("simulation succeeds");
        assert_eq!(report.state, PipelineState::Running);
        assert!(report.source.audio_frames > 90_000);
        assert!(report.sink.audio_frames > 90_000);
        assert!(report.input_ring.fill_frames <= report.input_ring.capacity_frames);
        assert!(report.output_ring.fill_frames <= report.output_ring.capacity_frames);
        assert!(
            route
                .sink()
                .captured_samples()
                .iter()
                .all(|sample| sample.is_finite())
        );
        route.stop().expect("route stops");
        assert_eq!(route.state(), PipelineState::Stopped);
    }

    #[test]
    fn independent_clock_drift_changes_callback_frame_counts() {
        let mut fast = route(10_000, Vec::new(), FakeFailureMode::Never);
        let mut slow = route(-10_000, Vec::new(), FakeFailureMode::Never);
        fast.start().expect("route starts");
        slow.start().expect("route starts");
        let fast_report = fast.run_for_ns(5_000_000_000).expect("simulation succeeds");
        let slow_report = slow.run_for_ns(5_000_000_000).expect("simulation succeeds");
        assert!(fast_report.source.audio_frames > slow_report.source.audio_frames);
        assert!(
            fast_report.output_ring.fill_frames > slow_report.output_ring.fill_frames
                || slow_report.output_ring.underrun_frames
                    > fast_report.output_ring.underrun_frames
        );
    }

    #[test]
    fn model_failure_falls_back_and_device_loss_fails_route() {
        let mut route = route(
            0,
            vec![FakeEndpointFault::DeviceLost { at_callback: 20 }],
            FakeFailureMode::OnCall(2),
        );
        route.start().expect("route starts");
        let report = route
            .run_for_ns(1_000_000_000)
            .expect("simulation handles injected faults");
        assert_eq!(report.state, PipelineState::Failed);
        assert_eq!(report.source.device_losses, 1);
        assert_eq!(report.reducer_failures, 1);
        route.stop().expect("failed route can stop");
    }
}
