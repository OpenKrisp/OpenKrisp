use super::*;
use audio_protocol::{DeviceId, DeviceKind, SampleFormat};
use backend::OwnedStream;
use denoise_core::{
    FakeFailureMode, FakeNoiseReducer, FakeNoiseReducerConfig, NoiseReducer, NoiseReductionStats,
};
use std::{collections::VecDeque, sync::Mutex, time::Duration};

#[derive(Default)]
struct Evidence {
    events: Mutex<Vec<String>>,
    issues: Mutex<Vec<StreamIssueStore>>,
    taps: Mutex<Vec<audio_cpal::AudioTapControl>>,
    workers: Mutex<Vec<Arc<AtomicBool>>>,
}
impl Evidence {
    fn record(&self, text: impl Into<String>) {
        self.events.lock().unwrap().push(text.into());
    }
}
struct Stream {
    label: String,
    evidence: Arc<Evidence>,
    _writer: audio_cpal::AudioTapWriter,
    tap: audio_cpal::AudioTapControl,
}
impl OwnedStream for Stream {
    fn close_audio_tap(&self) {
        self.tap.close();
    }
    fn callback_metrics(&self) -> CallbackMetricsSnapshot {
        CallbackMetricsSnapshot {
            callbacks: 10,
            frames: 4800,
        }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        self.tap.close();
        self.evidence.record(format!("drop:{}", self.label));
    }
}
struct Reducer {
    fake: FakeNoiseReducer,
    dropped: Arc<AtomicBool>,
    panic: bool,
}
impl NoiseReducer for Reducer {
    fn descriptor(&self) -> &NoiseReducerDescriptor {
        self.fake.descriptor()
    }
    fn process_planar(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<NoiseReductionStats, DenoiseError> {
        assert!(!self.panic, "injected worker panic");
        self.fake.process_planar(input, output)
    }
    fn reset(&mut self) -> Result<(), DenoiseError> {
        self.fake.reset()
    }
    fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
        self.fake.update_settings(settings)
    }
}
impl Drop for Reducer {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}
#[derive(Clone, Copy)]
enum Attempt {
    Ready,
    FailAfterInput,
    FailAfterOutput,
    Panic,
    Cancel,
}
struct FakeBackend {
    evidence: Arc<Evidence>,
    attempts: VecDeque<Attempt>,
    generation: usize,
}
fn device(id: &str, format: &AudioFormat) -> AudioDeviceDescriptor {
    AudioDeviceDescriptor {
        id: DeviceId::new(id).unwrap(),
        name: id.into(),
        host: "fake".into(),
        kind: DeviceKind::Physical,
        manufacturer: None,
        driver: None,
        supports_input: true,
        supports_output: true,
        is_default_input: false,
        is_default_output: false,
        default_input_format: Some(format.clone()),
        default_output_format: Some(format.clone()),
    }
}
impl FakeBackend {
    fn stream(&self, label: String, format: &AudioFormat, control: &MonitorControl) -> RouteStream {
        let (writer, _) = audio_cpal::audio_tap(
            format.clone(),
            StreamDirection::Input,
            Arc::new(AtomicBool::new(true)),
            control.cancelled.clone(),
        )
        .unwrap();
        let tap = writer.control();
        self.evidence.taps.lock().unwrap().push(tap.clone());
        self.evidence.record(format!("create:{label}"));
        RouteStream::new(Stream {
            label,
            evidence: self.evidence.clone(),
            _writer: writer,
            tap,
        })
    }
}
impl GenerationBackend for FakeBackend {
    fn build(
        &mut self,
        _config: &HeadlessMonitorConfig,
        _asset: &ModelAsset,
        control: &MonitorControl,
    ) -> Result<GenerationBuildOutcome, HeadlessMonitorError> {
        self.generation += 1;
        let attempt = self.attempts.pop_front().unwrap_or(Attempt::Ready);
        let format = AudioFormat {
            sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
            channels: ChannelCount::new(1).unwrap(),
            sample_format: SampleFormat::F32,
            buffer_size_frames: None,
        };
        let input_stream = self.stream(format!("{}:input", self.generation), &format, control);
        let unavailable =
            || HeadlessMonitorError::Routing(platform_routing::RoutingError::DeviceUnavailable);
        if matches!(attempt, Attempt::FailAfterInput) {
            return Err(unavailable());
        }
        let output_stream = self.stream(format!("{}:output", self.generation), &format, control);
        if matches!(attempt, Attempt::FailAfterOutput) {
            return Err(unavailable());
        }
        if matches!(attempt, Attempt::Cancel) {
            control.cancel();
            control.check()?;
        }
        let (mut producer, input) = bounded_audio_ring(4800, format.channels).unwrap();
        if matches!(attempt, Attempt::Panic) {
            producer.write(&[0.1; 480]).unwrap();
        }
        let (output, _) = bounded_audio_ring(9600, format.channels).unwrap();
        let metrics = RouteMetricsStore::new(
            input.metrics(),
            output.metrics(),
            format.clone(),
            format.clone(),
        );
        let descriptor = NoiseReducerDescriptor {
            model_id: "fault-injection".into(),
            sample_rate_hz: format.sample_rate_hz,
            channels: format.channels,
            frame_samples_per_channel: 480,
            algorithmic_latency_samples: 480,
        };
        let dropped = Arc::new(AtomicBool::new(false));
        self.evidence.workers.lock().unwrap().push(dropped.clone());
        let worker = crate::spawn_pipeline_worker(
            PipelineWorkerConfig::new(format.clone(), format.clone(), 4800),
            input,
            output,
            metrics.clone(),
            move || {
                Ok(Reducer {
                    fake: FakeNoiseReducer::new(FakeNoiseReducerConfig {
                        descriptor,
                        settings: DenoiseSettings::default(),
                        enabled_gain: 0.5,
                        processing_delay: Duration::ZERO,
                        failure_mode: FakeFailureMode::Never,
                        stats: NoiseReductionStats::default(),
                    })?,
                    dropped,
                    panic: matches!(attempt, Attempt::Panic),
                })
            },
        )
        .unwrap();
        let input_issues = StreamIssueStore::default();
        let output_issues = StreamIssueStore::default();
        self.evidence
            .issues
            .lock()
            .unwrap()
            .push(input_issues.clone());
        Ok(GenerationBuildOutcome {
            generation: RouteGeneration {
                input_device: device("fake:input", &format),
                output_device: device("fake:output", &format),
                input_format: format.clone(),
                output_format: format,
                input_stream,
                output_stream,
                worker,
                metrics,
                input_issues,
                output_issues,
            },
            recovery_attempts: 0,
            last_error: None,
        })
    }
}
fn setup(attempts: &[Attempt]) -> (Arc<Evidence>, Box<FakeBackend>, HeadlessMonitorConfig) {
    let evidence = Arc::new(Evidence::default());
    let backend = Box::new(FakeBackend {
        evidence: evidence.clone(),
        attempts: attempts.iter().copied().collect(),
        generation: 0,
    });
    let mut config = HeadlessMonitorConfig::default();
    config.recovery.initial_backoff = Duration::from_micros(1);
    config.recovery.max_backoff = Duration::from_micros(1);
    config.recovery.max_attempts = 3;
    (evidence, backend, config)
}
fn assert_clean(evidence: &Evidence) {
    let events = evidence.events.lock().unwrap();
    for created in events.iter().filter_map(|e| e.strip_prefix("create:")) {
        assert_eq!(
            events
                .iter()
                .filter(|e| *e == &format!("drop:{created}"))
                .count(),
            1,
            "{events:?}"
        );
    }
    assert!(
        evidence
            .taps
            .lock()
            .unwrap()
            .iter()
            .all(|tap| tap.is_closed())
    );
    assert!(
        evidence
            .workers
            .lock()
            .unwrap()
            .iter()
            .all(|worker| worker.load(Ordering::Acquire))
    );
}
#[test]
fn partial_startup_rolls_back_and_retries_without_leaking_streams() {
    let (evidence, backend, config) = setup(&[
        Attempt::FailAfterInput,
        Attempt::FailAfterOutput,
        Attempt::Ready,
    ]);
    let mut route =
        HeadlessMonitor::start_with_backend(config, MonitorControl::default(), backend).unwrap();
    assert_eq!(route.state.state(), PipelineState::Running);
    assert_eq!(route.recovery_attempts, 2);
    route.stop().unwrap();
    route.stop().unwrap();
    drop(route);
    assert_clean(&evidence);
}
#[test]
fn device_loss_replaces_generation_and_preserves_the_other_route() {
    let (evidence, backend, config) =
        setup(&[Attempt::Ready, Attempt::FailAfterOutput, Attempt::Ready]);
    let mut route =
        HeadlessMonitor::start_with_backend(config, MonitorControl::default(), backend).unwrap();
    let (other_evidence, other_backend, other_config) = setup(&[Attempt::Ready]);
    let mut other =
        HeadlessMonitor::start_with_backend(other_config, MonitorControl::default(), other_backend)
            .unwrap();
    evidence.issues.lock().unwrap()[0].record(StreamIssueKind::DeviceNotAvailable);
    route.poll_health();
    assert_eq!(route.state.state(), PipelineState::Running);
    assert_eq!(route.successful_recoveries, 1);
    assert!(evidence.taps.lock().unwrap()[0].is_closed());
    assert!(!evidence.taps.lock().unwrap().last().unwrap().is_closed());
    assert_eq!(other.state.state(), PipelineState::Running);
    route.stop().unwrap();
    other.stop().unwrap();
    assert_clean(&evidence);
    assert_clean(&other_evidence);
}
#[test]
fn exhausted_recovery_releases_every_generation_and_remains_stoppable() {
    let (evidence, backend, config) = setup(&[
        Attempt::Ready,
        Attempt::FailAfterInput,
        Attempt::FailAfterOutput,
        Attempt::FailAfterInput,
    ]);
    let mut route =
        HeadlessMonitor::start_with_backend(config, MonitorControl::default(), backend).unwrap();
    evidence.issues.lock().unwrap()[0].record(StreamIssueKind::DeviceNotAvailable);
    route.poll_health();
    assert_eq!(route.state.state(), PipelineState::Failed);
    assert_eq!(route.recovery_attempts, 3);
    route.stop().unwrap();
    assert_clean(&evidence);
}
#[test]
fn cancellation_during_recovery_cannot_publish_running_or_retain_pcm() {
    let (evidence, backend, config) = setup(&[Attempt::Ready, Attempt::Cancel]);
    let mut route =
        HeadlessMonitor::start_with_backend(config, MonitorControl::default(), backend).unwrap();
    evidence.issues.lock().unwrap()[0].record(StreamIssueKind::DeviceNotAvailable);
    route.poll_health();
    assert_ne!(route.state.state(), PipelineState::Running);
    route.stop().unwrap();
    assert_clean(&evidence);
}
#[test]
fn worker_panic_is_observed_joined_and_recovered() {
    let (evidence, backend, config) = setup(&[Attempt::Panic, Attempt::Ready]);
    let mut route =
        HeadlessMonitor::start_with_backend(config, MonitorControl::default(), backend).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !route.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    route.poll_health();
    assert_eq!(route.state.state(), PipelineState::Running);
    assert_eq!(route.successful_recoveries, 1);
    route.stop().unwrap();
    assert_clean(&evidence);
}
