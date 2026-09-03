use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use audio_protocol::ChannelCount;
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Observer, Producer, Split},
};

/// Construction or alignment failure for a bounded audio transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AudioTransportError {
    #[error("audio ring capacity must be greater than zero frames")]
    ZeroCapacity,
    #[error("audio ring capacity overflowed addressable memory")]
    CapacityOverflow,
    #[error("{samples} samples are not aligned to {channels} channels")]
    MisalignedBuffer { samples: usize, channels: usize },
}

/// Result of one non-blocking producer write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteReport {
    pub requested_frames: usize,
    pub written_frames: usize,
    pub dropped_frames: usize,
}

/// Result of one non-blocking consumer read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadReport {
    pub requested_frames: usize,
    pub read_frames: usize,
    pub silence_frames: usize,
}

#[derive(Debug)]
struct MetricAtoms {
    fill_frames: AtomicUsize,
    high_water_frames: AtomicUsize,
    low_water_frames: AtomicUsize,
    overrun_events: AtomicU64,
    dropped_frames: AtomicU64,
    stale_frames_discarded: AtomicU64,
    underrun_callbacks: AtomicU64,
    underrun_frames: AtomicU64,
    discontinuities: AtomicU64,
}

impl MetricAtoms {
    fn new(capacity_frames: usize) -> Self {
        Self {
            fill_frames: AtomicUsize::new(0),
            high_water_frames: AtomicUsize::new(0),
            low_water_frames: AtomicUsize::new(capacity_frames),
            overrun_events: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            stale_frames_discarded: AtomicU64::new(0),
            underrun_callbacks: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
            discontinuities: AtomicU64::new(0),
        }
    }

    fn record_fill(&self, fill_frames: usize) {
        self.fill_frames.store(fill_frames, Ordering::Relaxed);
        self.high_water_frames
            .fetch_max(fill_frames, Ordering::Relaxed);
        self.low_water_frames
            .fetch_min(fill_frames, Ordering::Relaxed);
    }
}

/// Cloneable read-only metrics handle shared by the ring endpoints.
#[derive(Clone, Debug)]
pub struct AudioRingMetrics {
    capacity_frames: usize,
    atoms: Arc<MetricAtoms>,
}

impl AudioRingMetrics {
    /// Takes an approximate lock-free snapshot suitable for diagnostics.
    #[must_use]
    pub fn snapshot(&self) -> AudioRingMetricsSnapshot {
        AudioRingMetricsSnapshot {
            capacity_frames: self.capacity_frames,
            fill_frames: self.atoms.fill_frames.load(Ordering::Relaxed),
            high_water_frames: self.atoms.high_water_frames.load(Ordering::Relaxed),
            low_water_frames: self.atoms.low_water_frames.load(Ordering::Relaxed),
            overrun_events: self.atoms.overrun_events.load(Ordering::Relaxed),
            dropped_frames: self.atoms.dropped_frames.load(Ordering::Relaxed),
            stale_frames_discarded: self.atoms.stale_frames_discarded.load(Ordering::Relaxed),
            underrun_callbacks: self.atoms.underrun_callbacks.load(Ordering::Relaxed),
            underrun_frames: self.atoms.underrun_frames.load(Ordering::Relaxed),
            discontinuities: self.atoms.discontinuities.load(Ordering::Relaxed),
        }
    }
}

/// Primitive health counters for one bounded audio ring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioRingMetricsSnapshot {
    pub capacity_frames: usize,
    pub fill_frames: usize,
    pub high_water_frames: usize,
    pub low_water_frames: usize,
    pub overrun_events: u64,
    pub dropped_frames: u64,
    pub stale_frames_discarded: u64,
    pub underrun_callbacks: u64,
    pub underrun_frames: u64,
    pub discontinuities: u64,
}

/// Single-producer, non-blocking writer for complete interleaved audio frames.
pub struct AudioRingProducer {
    inner: HeapProd<f32>,
    channels: ChannelCount,
    capacity_frames: usize,
    metrics: AudioRingMetrics,
}

impl AudioRingProducer {
    /// Writes as many complete frames as fit and drops the newest excess frames.
    pub fn write(&mut self, samples: &[f32]) -> Result<WriteReport, AudioTransportError> {
        ensure_aligned(samples.len(), self.channels)?;
        let channels = self.channels.as_usize();
        let requested_frames = samples.len() / channels;
        let written_frames = requested_frames.min(self.inner.vacant_len() / channels);
        let written_samples = written_frames * channels;
        let pushed = self.inner.push_slice(&samples[..written_samples]);
        debug_assert_eq!(pushed, written_samples);
        let dropped_frames = requested_frames - written_frames;
        if dropped_frames > 0 {
            self.metrics
                .atoms
                .overrun_events
                .fetch_add(1, Ordering::Relaxed);
            self.metrics
                .atoms
                .dropped_frames
                .fetch_add(dropped_frames as u64, Ordering::Relaxed);
            self.metrics
                .atoms
                .discontinuities
                .fetch_add(1, Ordering::Relaxed);
        }
        self.record_observed_fill();
        Ok(WriteReport {
            requested_frames,
            written_frames,
            dropped_frames,
        })
    }

    /// Returns a metrics handle without exposing the ring implementation.
    #[must_use]
    pub fn metrics(&self) -> AudioRingMetrics {
        self.metrics.clone()
    }

    /// Returns the fixed capacity in audio frames.
    #[must_use]
    pub const fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }

    fn record_observed_fill(&self) {
        self.metrics
            .atoms
            .record_fill(self.inner.occupied_len() / self.channels.as_usize());
    }
}

/// Single-consumer, non-blocking reader for complete interleaved audio frames.
pub struct AudioRingConsumer {
    inner: HeapCons<f32>,
    channels: ChannelCount,
    capacity_frames: usize,
    metrics: AudioRingMetrics,
}

impl AudioRingConsumer {
    /// Reads up to the output capacity without blocking or filling the remainder.
    pub fn read(&mut self, output: &mut [f32]) -> Result<ReadReport, AudioTransportError> {
        ensure_aligned(output.len(), self.channels)?;
        let channels = self.channels.as_usize();
        let requested_frames = output.len() / channels;
        let read_frames = requested_frames.min(self.inner.occupied_len() / channels);
        let read_samples = read_frames * channels;
        let popped = self.inner.pop_slice(&mut output[..read_samples]);
        debug_assert_eq!(popped, read_samples);
        self.record_observed_fill();
        Ok(ReadReport {
            requested_frames,
            read_frames,
            silence_frames: 0,
        })
    }

    /// Reads available frames and fills a short callback buffer with silence.
    pub fn read_or_silence(
        &mut self,
        output: &mut [f32],
    ) -> Result<ReadReport, AudioTransportError> {
        let mut report = self.read(output)?;
        let channels = self.channels.as_usize();
        report.silence_frames = report.requested_frames - report.read_frames;
        if report.silence_frames > 0 {
            output[report.read_frames * channels..].fill(0.0);
            self.metrics
                .atoms
                .underrun_callbacks
                .fetch_add(1, Ordering::Relaxed);
            self.metrics
                .atoms
                .underrun_frames
                .fetch_add(report.silence_frames as u64, Ordering::Relaxed);
        }
        Ok(report)
    }

    /// Discards oldest queued frames until fill is no greater than `max_fill_frames`.
    ///
    /// Calling this from the sole consumer implements the input stale-audio policy without making
    /// the producer race the consumer's read index.
    pub fn trim_oldest_to(&mut self, max_fill_frames: usize) -> usize {
        let channels = self.channels.as_usize();
        let fill_frames = self.inner.occupied_len() / channels;
        let discarded_frames = fill_frames.saturating_sub(max_fill_frames);
        if discarded_frames > 0 {
            let skipped = self.inner.skip(discarded_frames * channels);
            debug_assert_eq!(skipped, discarded_frames * channels);
            self.metrics
                .atoms
                .stale_frames_discarded
                .fetch_add(discarded_frames as u64, Ordering::Relaxed);
            self.metrics
                .atoms
                .discontinuities
                .fetch_add(1, Ordering::Relaxed);
        }
        self.record_observed_fill();
        discarded_frames
    }

    /// Returns a metrics handle without exposing the ring implementation.
    #[must_use]
    pub fn metrics(&self) -> AudioRingMetrics {
        self.metrics.clone()
    }

    /// Returns the fixed capacity in audio frames.
    #[must_use]
    pub const fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }

    fn record_observed_fill(&self) {
        self.metrics
            .atoms
            .record_fill(self.inner.occupied_len() / self.channels.as_usize());
    }
}

/// Allocates and splits one fixed-capacity SPSC ring.
pub fn bounded_audio_ring(
    capacity_frames: usize,
    channels: ChannelCount,
) -> Result<(AudioRingProducer, AudioRingConsumer), AudioTransportError> {
    if capacity_frames == 0 {
        return Err(AudioTransportError::ZeroCapacity);
    }
    let capacity_samples = capacity_frames
        .checked_mul(channels.as_usize())
        .ok_or(AudioTransportError::CapacityOverflow)?;
    let (producer, consumer) = HeapRb::<f32>::new(capacity_samples).split();
    let metrics = AudioRingMetrics {
        capacity_frames,
        atoms: Arc::new(MetricAtoms::new(capacity_frames)),
    };
    Ok((
        AudioRingProducer {
            inner: producer,
            channels,
            capacity_frames,
            metrics: metrics.clone(),
        },
        AudioRingConsumer {
            inner: consumer,
            channels,
            capacity_frames,
            metrics,
        },
    ))
}

fn ensure_aligned(samples: usize, channels: ChannelCount) -> Result<(), AudioTransportError> {
    let channels = channels.as_usize();
    if !samples.is_multiple_of(channels) {
        return Err(AudioTransportError::MisalignedBuffer { samples, channels });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> ChannelCount {
        ChannelCount::new(2).expect("valid channel count")
    }

    #[test]
    fn capacity_is_frame_aware_and_never_grows() {
        let (mut producer, mut consumer) = bounded_audio_ring(3, stereo()).expect("valid ring");
        let report = producer
            .write(&[1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0])
            .expect("aligned input");
        assert_eq!(
            report,
            WriteReport {
                requested_frames: 4,
                written_frames: 3,
                dropped_frames: 1,
            }
        );
        assert_eq!(producer.capacity_frames(), 3);

        let mut output = [0.0; 6];
        assert_eq!(
            consumer
                .read(&mut output)
                .expect("aligned output")
                .read_frames,
            3
        );
        assert_eq!(output, [1.0, 10.0, 2.0, 20.0, 3.0, 30.0]);
        let metrics = producer.metrics().snapshot();
        assert_eq!(metrics.capacity_frames, 3);
        assert_eq!(metrics.high_water_frames, 3);
        assert_eq!(metrics.overrun_events, 1);
        assert_eq!(metrics.dropped_frames, 1);
    }

    #[test]
    fn consumer_discards_oldest_stale_frames() {
        let channels = ChannelCount::new(1).expect("valid channel count");
        let (mut producer, mut consumer) = bounded_audio_ring(8, channels).expect("valid ring");
        producer
            .write(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0])
            .expect("aligned input");
        assert_eq!(consumer.trim_oldest_to(3), 3);
        let mut latest = [0.0; 3];
        consumer.read(&mut latest).expect("aligned output");
        assert_eq!(latest, [3.0, 4.0, 5.0]);
        let metrics = consumer.metrics().snapshot();
        assert_eq!(metrics.stale_frames_discarded, 3);
        assert_eq!(metrics.discontinuities, 1);
    }

    #[test]
    fn output_underrun_is_safely_filled_once_per_callback() {
        let channels = ChannelCount::new(1).expect("valid channel count");
        let (mut producer, mut consumer) = bounded_audio_ring(4, channels).expect("valid ring");
        producer.write(&[0.25, 0.5]).expect("aligned input");
        let mut output = [9.0; 4];
        let report = consumer
            .read_or_silence(&mut output)
            .expect("aligned output");
        assert_eq!(report.read_frames, 2);
        assert_eq!(report.silence_frames, 2);
        assert_eq!(output, [0.25, 0.5, 0.0, 0.0]);
        let metrics = consumer.metrics().snapshot();
        assert_eq!(metrics.underrun_callbacks, 1);
        assert_eq!(metrics.underrun_frames, 2);
    }

    #[test]
    fn randomized_chunks_preserve_order_without_overflow() {
        let channels = ChannelCount::new(1).expect("valid channel count");
        let (mut producer, mut consumer) = bounded_audio_ring(64, channels).expect("valid ring");
        let input: Vec<f32> = (0..10_000).map(|value| value as f32).collect();
        let mut output = Vec::with_capacity(input.len());
        let mut input_offset = 0;
        let mut random = 0x42_u32;
        let mut scratch = [0.0; 31];
        while input_offset < input.len() {
            random = random.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let write_frames = 1 + random as usize % 29;
            let write_end = (input_offset + write_frames).min(input.len());
            let report = producer
                .write(&input[input_offset..write_end])
                .expect("aligned input");
            input_offset += report.written_frames;
            assert_eq!(report.dropped_frames, 0);

            let mut frames_left = report.written_frames;
            while frames_left > 0 {
                random = random.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let read_frames = (1 + random as usize % scratch.len()).min(frames_left);
                let report = consumer
                    .read(&mut scratch[..read_frames])
                    .expect("aligned output");
                assert_eq!(report.read_frames, read_frames);
                output.extend_from_slice(&scratch[..report.read_frames]);
                frames_left -= report.read_frames;
            }
        }
        assert_eq!(output, input);
        assert!(producer.metrics().snapshot().fill_frames <= producer.capacity_frames());
    }
}
