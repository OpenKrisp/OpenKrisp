use super::*;
use crate::{
    ApplicationPcmSource, PipelineWorkerConfig, PipelineWorkerHandle, RouteMetricsStore,
    bounded_audio_ring, lifecycle::MonitorControl, worker::spawn_pipeline_worker_controlled,
};
use audio_cpal::{CpalStream, NegotiationPreferences, StreamIssueStore};
use denoise_core::IndependentChannels;
use denoise_dfn::{DfnNoiseReducer, ModelAsset};

pub(super) struct ApplicationRoute {
    snapshot: RouteSnapshot,
    worker: Option<PipelineWorkerHandle>,
    output: Option<CpalStream>,
    metrics: RouteMetricsStore,
    issues: StreamIssueStore,
    accepting: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    started: Instant,
}

impl ApplicationRoute {
    pub(super) fn start(
        config: RouteConfig,
        source: ApplicationPcmSource,
        model_path: PathBuf,
        pressure: Arc<AtomicBool>,
        control: MonitorControl,
        audio_taps: crate::NativeAudioTaps,
    ) -> Result<Self, OperationFailure> {
        let build = || -> Result<Self, HeadlessMonitorError> {
            control.check()?;
            let host = CpalHost::new_default();
            let inventory = host.inventory()?;
            let destination =
                platform_routing::incoming::physical_destination(&config, &inventory)?;
            let device = host.resolve_output(&DeviceSelection::Id(destination.id.clone()))?;
            let negotiated = device.negotiate_output(NegotiationPreferences::new(
                source.format.sample_rate_hz,
                source.format.channels,
            ))?;
            let output_format = negotiated.protocol().clone();
            if source.format.channels.get() == 2 && output_format.channels.get() != 2 {
                return Err(HeadlessMonitorError::InvalidConfiguration(
                    "Stereo incoming PCM requires a stereo output",
                ));
            }
            let target = (u64::from(output_format.sample_rate_hz.get())
                * u64::from(config.target_latency_ms.get())
                / 1000)
                .max(1) as usize;
            let (mut producer, mut consumer) = bounded_audio_ring(
                target.max(output_format.sample_rate_hz.get() as usize / 10) * 2,
                output_format.channels,
            )?;
            producer.write(&vec![0.0; target * output_format.channels.as_usize()])?;
            let metrics = RouteMetricsStore::new(
                source.consumer.metrics(),
                producer.metrics(),
                source.format.clone(),
                output_format.clone(),
            );
            let enabled = Arc::new(AtomicBool::new(false));
            let callback_enabled = enabled.clone();
            let cancelled = control.cancelled.clone();
            let issues = StreamIssueStore::default();
            let callback_issues = issues.clone();
            let (tap, reader) = audio_cpal::audio_tap(
                output_format.clone(),
                audio_cpal::StreamDirection::Output,
                enabled.clone(),
                control.cancelled.clone(),
            )?;
            audio_taps.register(
                &config.id,
                true,
                audio_cpal::StreamDirection::Output,
                reader,
            );
            let output = device.build_output_stream_with_tap(
                &negotiated,
                move |samples| {
                    if cancelled.load(Ordering::Acquire)
                        || !callback_enabled.load(Ordering::Acquire)
                    {
                        samples.fill(0.0);
                        return;
                    }
                    if consumer.read_or_silence(samples).is_err() {
                        samples.fill(0.0);
                        callback_issues.record_callback_violation();
                    }
                },
                issues.clone(),
                Some(tap),
            )?;
            output.play()?;
            let deadline = Instant::now() + Duration::from_secs(3);
            while output.callback_metrics().callbacks == 0 {
                control.wait(Duration::from_millis(5))?;
                if issues.snapshot().terminal > 0 || Instant::now() >= deadline {
                    return Err(HeadlessMonitorError::ActivationTimeout {
                        direction: audio_cpal::StreamDirection::Output,
                    });
                }
            }
            let mut worker_config = PipelineWorkerConfig::new(
                source.format.clone(),
                output_format.clone(),
                source.format.sample_rate_hz.get() as usize / 20,
            );
            worker_config.incoming = true;
            worker_config.initially_bypassed =
                config.processing_mode == ProcessingMode::AlignedBypass;
            worker_config.denoise_enabled = config.denoise.enabled;
            worker_config.priority_pressure = pressure;
            worker_config.output_target_fill_frames = target;
            let asset = ModelAsset::load(model_path)?;
            let settings = config.denoise.clone();
            let channels = source.format.channels;
            let worker = spawn_pipeline_worker_controlled(
                worker_config,
                source.consumer.into(),
                producer,
                metrics.clone(),
                move || {
                    IndependentChannels::new(channels, || {
                        let mut reducer =
                            DfnNoiseReducer::from_asset(asset.clone(), settings.clone())?;
                        reducer.warm_up(3)?;
                        Ok(reducer)
                    })
                },
                &control.cancelled,
            )?;
            control.check()?;
            let descriptor = worker.descriptor();
            let mut snapshot = RouteSnapshot::pending(config, PipelineState::Running);
            snapshot.input_format = Some(source.format);
            snapshot.output_format = Some(output_format);
            snapshot.output_device = Some(destination.clone());
            snapshot.model = Some(ModelInfo {
                id: descriptor.model_id.clone(),
                sample_rate_hz: descriptor.sample_rate_hz,
                channels: descriptor.channels.get(),
                frame_samples_per_channel: saturating_u32(descriptor.frame_samples_per_channel),
                algorithmic_latency_samples: saturating_u32(descriptor.algorithmic_latency_samples),
            });
            source.accepting.store(true, Ordering::Release);
            enabled.store(true, Ordering::Release);
            Ok(Self {
                snapshot,
                worker: Some(worker),
                output: Some(output),
                metrics,
                issues,
                accepting: source.accepting,
                connected: source.connected,
                enabled,
                started: Instant::now(),
            })
        };
        build().map_err(OperationFailure::from_headless)
    }
}

impl ManagedRoute for ApplicationRoute {
    fn route_id(&self) -> &RouteId {
        &self.snapshot.config.id
    }
    fn snapshot(&mut self) -> RouteSnapshot {
        self.snapshot.metrics = self.metrics.snapshot();
        self.snapshot.model_errors = self.metrics.model_errors();
        self.snapshot.uptime_seconds = self.started.elapsed().as_secs_f64();
        if let Some(output) = &self.output {
            self.snapshot.output_stream =
                stream_diagnostics(output.callback_metrics(), self.issues.snapshot());
        }
        self.snapshot.clone()
    }
    fn set_bypass(&mut self, bypassed: bool) -> Result<(), OperationFailure> {
        if let Some(worker) = &self.worker {
            worker
                .set_bypass(bypassed)
                .map_err(|error| OperationFailure::from_headless(error.into()))?;
            self.snapshot.bypassed = bypassed;
            self.snapshot.config.processing_mode = if bypassed {
                ProcessingMode::AlignedBypass
            } else {
                ProcessingMode::SpeechDenoise
            };
        }
        Ok(())
    }
    fn update_settings(&mut self, settings: DenoiseSettings) -> Result<(), OperationFailure> {
        if let Some(worker) = &self.worker {
            worker
                .update_settings(settings.clone())
                .map_err(|error| OperationFailure::from_headless(error.into()))?;
            self.snapshot.config.denoise = settings;
        }
        Ok(())
    }
    fn poll_health(&mut self) {
        if self.snapshot.state != PipelineState::Running {
            return;
        }
        let fault = if !self.connected.load(Ordering::Acquire) {
            Some((
                ErrorCode::StreamError,
                "The application PCM producer disconnected.",
            ))
        } else if self
            .worker
            .as_ref()
            .is_none_or(PipelineWorkerHandle::is_finished)
        {
            Some((
                ErrorCode::WorkerTerminated,
                "The incoming audio worker stopped.",
            ))
        } else if self.issues.snapshot().terminal > 0 {
            Some((
                ErrorCode::StreamError,
                "The incoming audio output disconnected. Select the output and restart the route.",
            ))
        } else {
            None
        };
        if let Some((code, message)) = fault {
            let _ = self.stop();
            self.snapshot.state = PipelineState::Failed;
            self.snapshot.fault = Some(EngineErrorDto::new(code, message));
        }
    }
    fn stop(&mut self) -> Result<(), OperationFailure> {
        self.accepting.store(false, Ordering::Release);
        self.enabled.store(false, Ordering::Release);
        if let Some(output) = &self.output {
            output.close_audio_tap();
        }
        let result = self
            .worker
            .take()
            .map(|mut worker| worker.shutdown())
            .transpose();
        self.output.take();
        self.snapshot.state = PipelineState::Stopped;
        result
            .map(|_| ())
            .map_err(|error| OperationFailure::from_headless(error.into()))
    }
}

impl Drop for ApplicationRoute {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
