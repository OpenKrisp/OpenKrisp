use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use audio_protocol::{AudioFormat, PipelineMetrics};

use crate::AudioRingMetrics;

#[derive(Debug, Default)]
struct RouteMetricAtoms {
    input_peak: AtomicU32,
    input_rms: AtomicU32,
    output_peak: AtomicU32,
    output_rms: AtomicU32,
    inference_last_us: AtomicU64,
    inference_p95_us: AtomicU64,
    inference_p99_us: AtomicU64,
    missed_deadlines: AtomicU64,
    model_resets: AtomicU64,
    reset_last_us: AtomicU64,
    reset_total_us: AtomicU64,
    model_errors: AtomicU64,
    drift_correction_ppm: AtomicI32,
    degraded: AtomicBool,
    overload_events: AtomicU64,
    skipped_inferences: AtomicU64,
    fixed_latency_us: AtomicU64,
}

/// Thread-safe low-rate view over worker and callback health.
#[derive(Clone, Debug)]
pub struct RouteMetricsStore {
    atoms: Arc<RouteMetricAtoms>,
    input_ring: AudioRingMetrics,
    output_ring: AudioRingMetrics,
    input_format: AudioFormat,
    output_format: AudioFormat,
}

impl RouteMetricsStore {
    #[must_use]
    pub fn new(
        input_ring: AudioRingMetrics,
        output_ring: AudioRingMetrics,
        input_format: AudioFormat,
        output_format: AudioFormat,
    ) -> Self {
        Self {
            atoms: Arc::new(RouteMetricAtoms::default()),
            input_ring,
            output_ring,
            input_format,
            output_format,
        }
    }

    /// Builds a serializable snapshot without locking worker or callback threads.
    #[must_use]
    pub fn snapshot(&self) -> PipelineMetrics {
        let input_ring = self.input_ring.snapshot();
        let output_ring = self.output_ring.snapshot();
        let queue_latency_us = frames_to_us(
            input_ring.fill_frames,
            self.input_format.sample_rate_hz.get(),
        )
        .saturating_add(frames_to_us(
            output_ring.fill_frames,
            self.output_format.sample_rate_hz.get(),
        ));
        let estimated_latency_us = self
            .atoms
            .fixed_latency_us
            .load(Ordering::Relaxed)
            .saturating_add(queue_latency_us);
        PipelineMetrics {
            input_peak: load_f32(&self.atoms.input_peak),
            input_rms: load_f32(&self.atoms.input_rms),
            output_peak: load_f32(&self.atoms.output_peak),
            output_rms: load_f32(&self.atoms.output_rms),
            inference_last_us: self.atoms.inference_last_us.load(Ordering::Relaxed),
            inference_p95_us: self.atoms.inference_p95_us.load(Ordering::Relaxed),
            inference_p99_us: self.atoms.inference_p99_us.load(Ordering::Relaxed),
            missed_deadlines: self.atoms.missed_deadlines.load(Ordering::Relaxed),
            input_ring_fill_frames: saturating_u32(input_ring.fill_frames),
            output_ring_fill_frames: saturating_u32(output_ring.fill_frames),
            input_ring_high_water_frames: saturating_u32(input_ring.high_water_frames),
            output_ring_low_water_frames: saturating_u32(output_ring.low_water_frames),
            input_overruns: input_ring.overrun_events,
            output_underruns: output_ring.underrun_callbacks,
            discontinuities: input_ring
                .discontinuities
                .saturating_add(output_ring.discontinuities),
            model_resets: self.atoms.model_resets.load(Ordering::Relaxed),
            reset_last_us: self.atoms.reset_last_us.load(Ordering::Relaxed),
            reset_total_us: self.atoms.reset_total_us.load(Ordering::Relaxed),
            drift_correction_ppm: self.atoms.drift_correction_ppm.load(Ordering::Relaxed),
            degraded: self.atoms.degraded.load(Ordering::Relaxed),
            overload_events: self.atoms.overload_events.load(Ordering::Relaxed),
            skipped_inferences: self.atoms.skipped_inferences.load(Ordering::Relaxed),
            estimated_latency_ms: estimated_latency_us as f32 / 1_000.0,
        }
    }

    #[must_use]
    pub fn model_errors(&self) -> u64 {
        self.atoms.model_errors.load(Ordering::Relaxed)
    }

    pub(crate) fn set_fixed_latency(&self, latency: Duration) {
        self.atoms
            .fixed_latency_us
            .store(duration_us(latency), Ordering::Relaxed);
    }

    pub(crate) fn record_input_level(&self, peak: f32, rms: f32) {
        store_f32(&self.atoms.input_peak, peak);
        store_f32(&self.atoms.input_rms, rms);
    }

    pub(crate) fn record_output_level(&self, peak: f32, rms: f32) {
        store_f32(&self.atoms.output_peak, peak);
        store_f32(&self.atoms.output_rms, rms);
    }

    pub(crate) fn record_inference(
        &self,
        elapsed: Duration,
        deadline: Duration,
        p95_us: u64,
        p99_us: u64,
    ) {
        self.atoms
            .inference_last_us
            .store(duration_us(elapsed), Ordering::Relaxed);
        self.atoms.inference_p95_us.store(p95_us, Ordering::Relaxed);
        self.atoms.inference_p99_us.store(p99_us, Ordering::Relaxed);
        if elapsed > deadline {
            self.atoms.missed_deadlines.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_model_error(&self) {
        self.atoms.model_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_model_reset(&self) {
        self.atoms.model_resets.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_reset_duration(&self, elapsed: Duration) {
        let micros = duration_us(elapsed);
        self.atoms.reset_last_us.store(micros, Ordering::Relaxed);
        self.atoms
            .reset_total_us
            .fetch_add(micros, Ordering::Relaxed);
    }

    pub(crate) fn record_drift_correction(&self, correction_ppm: i32) {
        self.atoms
            .drift_correction_ppm
            .store(correction_ppm, Ordering::Relaxed);
    }

    pub(crate) fn set_degraded(&self, degraded: bool) {
        let was_degraded = self.atoms.degraded.swap(degraded, Ordering::Relaxed);
        if degraded && !was_degraded {
            self.atoms.overload_events.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_skipped_inference(&self) {
        self.atoms
            .skipped_inferences
            .fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn output_fill_frames(&self) -> usize {
        self.output_ring.fill_frames()
    }

    #[must_use]
    pub(crate) fn input_discontinuities(&self) -> u64 {
        self.input_ring.discontinuities()
    }

    #[must_use]
    pub(crate) fn output_discontinuities(&self) -> u64 {
        self.output_ring.discontinuities()
    }
}

fn duration_us(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn frames_to_us(frames: usize, sample_rate_hz: u32) -> u64 {
    let numerator = (frames as u128).saturating_mul(1_000_000);
    u64::try_from(numerator / u128::from(sample_rate_hz)).unwrap_or(u64::MAX)
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn store_f32(target: &AtomicU32, value: f32) {
    target.store(value.to_bits(), Ordering::Relaxed);
}

fn load_f32(source: &AtomicU32) -> f32 {
    f32::from_bits(source.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use audio_protocol::{ChannelCount, SampleFormat, SampleRateHz};

    use super::*;
    use crate::bounded_audio_ring;

    fn format(rate: u32, channels: u16) -> AudioFormat {
        AudioFormat {
            sample_rate_hz: SampleRateHz::new(rate).expect("valid rate"),
            channels: ChannelCount::new(channels).expect("valid channels"),
            sample_format: SampleFormat::F32,
            buffer_size_frames: None,
        }
    }

    #[test]
    fn snapshot_combines_worker_and_ring_metrics() {
        let channels = ChannelCount::new(1).expect("valid channels");
        let (mut input_producer, input_consumer) =
            bounded_audio_ring(480, channels).expect("valid input ring");
        let (output_producer, mut output_consumer) =
            bounded_audio_ring(480, channels).expect("valid output ring");
        let metrics = RouteMetricsStore::new(
            input_consumer.metrics(),
            output_producer.metrics(),
            format(48_000, 1),
            format(48_000, 1),
        );
        input_producer.write(&[0.0; 480]).expect("input write");
        let mut underrun = [0.0; 10];
        output_consumer
            .read_or_silence(&mut underrun)
            .expect("output read");
        metrics.set_fixed_latency(Duration::from_millis(10));
        metrics.record_input_level(0.5, 0.25);
        metrics.record_output_level(0.4, 0.2);
        metrics.record_inference(
            Duration::from_millis(11),
            Duration::from_millis(10),
            9_000,
            11_000,
        );
        metrics.record_drift_correction(-42);
        metrics.set_degraded(true);
        metrics.set_degraded(true);
        metrics.record_skipped_inference();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.input_peak, 0.5);
        assert_eq!(snapshot.output_rms, 0.2);
        assert_eq!(snapshot.input_ring_fill_frames, 480);
        assert_eq!(snapshot.output_underruns, 1);
        assert_eq!(snapshot.missed_deadlines, 1);
        assert_eq!(snapshot.drift_correction_ppm, -42);
        assert!(snapshot.degraded);
        assert_eq!(snapshot.overload_events, 1);
        assert_eq!(snapshot.skipped_inferences, 1);
        assert!((snapshot.estimated_latency_ms - 20.0).abs() < 0.01);
    }
}
