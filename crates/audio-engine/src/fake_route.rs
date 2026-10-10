use std::time::Duration;

use crate::{
    AudioRingConsumer, AudioRingMetrics, AudioRingMetricsSnapshot, AudioRingProducer,
    AudioTransportError, CallbackEvent, FakeAudioSink, FakeAudioSource, FakeEndpointError,
    FakeEndpointStats, PipelineWorkerConfig, RouteMetricsStore, WorkerError, bounded_audio_ring,
    worker::{PipelineGraph, WorkerInput},
};
use audio_protocol::{
    AudioFormat, ChannelCount, PipelineMetrics, PipelineState, SampleFormat, SampleRateHz,
};
use denoise_core::{DenoiseError, NoiseReducer};

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
    #[error(transparent)]
    Worker(#[from] WorkerError),
}

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

/// Independent fake callback clocks drive the same processing core as the production worker.
/// Endpoint events and inference costs are deterministic; no physical audio device is opened.
pub struct SimulatedRoute<R: NoiseReducer> {
    state: PipelineState,
    source: FakeAudioSource,
    sink: FakeAudioSink,
    reducer: R,
    graph: PipelineGraph,
    input_producer: AudioRingProducer,
    input: WorkerInput,
    output_producer: AudioRingProducer,
    output_consumer: AudioRingConsumer,
    input_metrics: AudioRingMetrics,
    output_metrics: AudioRingMetrics,
    metrics: RouteMetricsStore,
    source_scratch: Vec<f32>,
    sink_scratch: Vec<f32>,
    elapsed_ns: u64,
}

fn format(rate: SampleRateHz, channels: ChannelCount) -> AudioFormat {
    AudioFormat {
        sample_rate_hz: rate,
        channels,
        sample_format: SampleFormat::F32,
        buffer_size_frames: None,
    }
}

impl<R: NoiseReducer> SimulatedRoute<R> {
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
        let mut config = PipelineWorkerConfig::new(
            format(source_rate, source_channels),
            format(sink_rate, sink_channels),
            input_target_fill_frames,
        );
        config.output_target_fill_frames = output_prefill_frames.max(1);
        Self::with_config(
            source,
            sink,
            reducer,
            input_capacity_frames,
            output_capacity_frames,
            output_prefill_frames,
            config,
        )
    }

    /// Uses the full production configuration, including stereo incoming and overload priority.
    pub fn with_config(
        source: FakeAudioSource,
        sink: FakeAudioSink,
        reducer: R,
        input_capacity_frames: usize,
        output_capacity_frames: usize,
        output_prefill_frames: usize,
        config: PipelineWorkerConfig,
    ) -> Result<Self, SimulationError> {
        if config.input_stale_target_frames > input_capacity_frames {
            return Err(SimulationError::InputTargetExceedsCapacity);
        }
        if output_prefill_frames > output_capacity_frames {
            return Err(SimulationError::OutputPrefillExceedsCapacity);
        }
        if source.format()
            != (
                config.input_format.sample_rate_hz,
                config.input_format.channels,
            )
            || sink.format()
                != (
                    config.output_format.sample_rate_hz,
                    config.output_format.channels,
                )
        {
            return Err(SimulationError::ChannelCountMismatch);
        }
        let source_scratch = vec![0.; source.max_callback_frames() * source.format().1.as_usize()];
        let sink_scratch = vec![0.; sink.max_callback_frames() * sink.format().1.as_usize()];
        let (input_producer, consumer) =
            bounded_audio_ring(input_capacity_frames, config.input_format.channels)?;
        let (mut output_producer, output_consumer) =
            bounded_audio_ring(output_capacity_frames, config.output_format.channels)?;
        let input_metrics = input_producer.metrics();
        let output_metrics = output_producer.metrics();
        let metrics = RouteMetricsStore::new(
            input_metrics.clone(),
            output_metrics.clone(),
            config.input_format.clone(),
            config.output_format.clone(),
        );
        let mut graph = PipelineGraph::new(
            &config,
            reducer.descriptor(),
            &metrics,
            output_capacity_frames,
        )?;
        graph.set_processing_cost(Duration::ZERO);
        output_producer.write(&vec![
            0.;
            output_prefill_frames
                * config.output_format.channels.as_usize()
        ])?;
        Ok(Self {
            state: PipelineState::Stopped,
            source,
            sink,
            reducer,
            graph,
            input_producer,
            input: consumer.into(),
            output_producer,
            output_consumer,
            input_metrics,
            output_metrics,
            metrics,
            source_scratch,
            sink_scratch,
            elapsed_ns: 0,
        })
    }

    pub fn start(&mut self) -> Result<(), SimulationError> {
        if self.state != PipelineState::Stopped {
            return Err(SimulationError::InvalidState(self.state));
        }
        self.state = PipelineState::Running;
        Ok(())
    }

    pub fn run_for_ns(&mut self, duration_ns: u64) -> Result<SimulationReport, SimulationError> {
        if self.state != PipelineState::Running {
            return Err(SimulationError::InvalidState(self.state));
        }
        let target_ns = self
            .elapsed_ns
            .checked_add(duration_ns)
            .ok_or(SimulationError::DurationOverflow)?;
        loop {
            let source = self.source.next_deadline_ns();
            let sink = self.sink.next_deadline_ns();
            let next = match (source, sink) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => break,
            };
            if next > target_ns {
                break;
            }
            if source.is_some_and(|s| s <= sink.unwrap_or(u64::MAX)) {
                self.run_source_callback()?;
            } else {
                self.run_sink_callback()?;
            }
            self.elapsed_ns = next;
            if self.state != PipelineState::Running {
                break;
            }
        }
        if self.state == PipelineState::Running {
            self.elapsed_ns = target_ns;
        }
        Ok(self.report())
    }

    pub fn stop(&mut self) -> Result<(), SimulationError> {
        if !matches!(self.state, PipelineState::Running | PipelineState::Failed) {
            return Err(SimulationError::InvalidState(self.state));
        }
        self.state = PipelineState::Stopping;
        self.graph.reset(&mut self.reducer, &self.metrics);
        self.state = PipelineState::Stopped;
        Ok(())
    }
    #[must_use]
    pub const fn state(&self) -> PipelineState {
        self.state
    }
    #[must_use]
    pub const fn sink(&self) -> &FakeAudioSink {
        &self.sink
    }
    #[must_use]
    pub fn metrics(&self) -> PipelineMetrics {
        self.metrics.snapshot()
    }
    pub fn set_processing_cost(&mut self, cost: Duration) {
        self.graph.set_processing_cost(cost);
    }
    pub fn set_source_drift_ppm(&mut self, ppm: i32) -> Result<(), SimulationError> {
        self.source.set_drift_ppm(ppm)?;
        Ok(())
    }
    #[must_use]
    pub fn report(&self) -> SimulationReport {
        SimulationReport {
            state: self.state,
            elapsed_ns: self.elapsed_ns,
            source: self.source.stats(),
            sink: self.sink.stats(),
            input_ring: self.input_metrics.snapshot(),
            output_ring: self.output_metrics.snapshot(),
            reducer_failures: self.metrics.model_errors(),
        }
    }
    fn run_source_callback(&mut self) -> Result<(), SimulationError> {
        match self.source.next_callback(&mut self.source_scratch)? {
            CallbackEvent::Audio(callback) => {
                let samples = callback.frames * self.source.format().1.as_usize();
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
                let samples = callback.frames * self.sink.format().1.as_usize();
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
        while self.graph.step(
            &mut self.reducer,
            &mut self.input,
            &mut self.output_producer,
            &self.metrics,
        )? {}
        Ok(())
    }
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
        assert!(report.reducer_failures > 0);
        assert_eq!(route.metrics().model_resets, report.reducer_failures);
        route.stop().expect("failed route can stop");
    }
}
