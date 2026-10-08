use audio_protocol::{ChannelCount, SampleRateHz};

/// Invalid fake-endpoint configuration or callback use.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FakeEndpointError {
    #[error("callback-size pattern must contain at least one entry")]
    EmptyCallbackPattern,
    #[error("callback sizes must be greater than zero frames")]
    ZeroCallbackSize,
    #[error("clock drift must be greater than -1,000,000 ppm")]
    InvalidDriftPpm,
    #[error("callback buffer needs {required} samples but has {actual}")]
    CallbackBufferTooSmall { required: usize, actual: usize },
    #[error("callback buffer size overflowed addressable memory")]
    BufferSizeOverflow,
    #[error("endpoint is disconnected")]
    Disconnected,
    #[error("the previous sink callback has not been consumed")]
    PendingSinkCallback,
    #[error("the sink has no pending audio callback")]
    NoPendingSinkCallback,
    #[error("sink callback expected {expected} samples but received {actual}")]
    SinkBufferLength { expected: usize, actual: usize },
}

/// Repeatable callback sizes and deterministic scheduling jitter.
#[derive(Clone, Debug)]
pub struct CallbackSchedule {
    sizes_frames: Vec<usize>,
    jitter_ns: Vec<i64>,
    cursor: usize,
}

impl CallbackSchedule {
    /// Creates a cyclic schedule. An empty jitter pattern means zero jitter.
    pub fn new(sizes_frames: Vec<usize>, jitter_ns: Vec<i64>) -> Result<Self, FakeEndpointError> {
        if sizes_frames.is_empty() {
            return Err(FakeEndpointError::EmptyCallbackPattern);
        }
        if sizes_frames.contains(&0) {
            return Err(FakeEndpointError::ZeroCallbackSize);
        }
        Ok(Self {
            sizes_frames,
            jitter_ns,
            cursor: 0,
        })
    }

    /// Convenience constructor for a zero-jitter cyclic size pattern.
    pub fn without_jitter(sizes_frames: Vec<usize>) -> Result<Self, FakeEndpointError> {
        Self::new(sizes_frames, Vec::new())
    }

    fn peek(&self) -> (usize, i64) {
        let frames = self.sizes_frames[self.cursor % self.sizes_frames.len()];
        let jitter_ns = if self.jitter_ns.is_empty() {
            0
        } else {
            self.jitter_ns[self.cursor % self.jitter_ns.len()]
        };
        (frames, jitter_ns)
    }

    fn advance(&mut self) -> (usize, i64) {
        let next = self.peek();
        self.cursor += 1;
        next
    }

    /// Returns the largest callback in the pattern.
    #[must_use]
    pub fn max_frames(&self) -> usize {
        self.sizes_frames.iter().copied().max().unwrap_or(0)
    }
}

/// Exact rational clock used by fake endpoints.
#[derive(Clone, Debug)]
pub struct SimulatedClock {
    sample_rate_hz: SampleRateHz,
    drift_ppm: i32,
    elapsed_numerator: u128,
    denominator: u128,
    elapsed_ns: u64,
}

impl SimulatedClock {
    /// Creates a clock whose actual rate is `nominal * (1 + drift_ppm / 1_000_000)`.
    pub fn new(sample_rate_hz: SampleRateHz, drift_ppm: i32) -> Result<Self, FakeEndpointError> {
        let drift_scale = 1_000_000_i64 + i64::from(drift_ppm);
        if drift_scale <= 0 {
            return Err(FakeEndpointError::InvalidDriftPpm);
        }
        Ok(Self {
            sample_rate_hz,
            drift_ppm,
            elapsed_numerator: 0,
            denominator: u128::from(sample_rate_hz.get()) * drift_scale as u128,
            elapsed_ns: 0,
        })
    }

    /// Predicts the deadline after a number of additional frames without advancing the clock.
    #[must_use]
    pub fn deadline_after_frames(&self, frames: usize) -> u64 {
        let numerator = self
            .elapsed_numerator
            .saturating_add((frames as u128).saturating_mul(1_000_000_000_000_000));
        u64::try_from(numerator / self.denominator).unwrap_or(u64::MAX)
    }

    /// Advances by complete audio frames and returns the resulting ideal timestamp.
    pub fn advance_frames(&mut self, frames: usize) -> u64 {
        self.elapsed_numerator = self
            .elapsed_numerator
            .saturating_add((frames as u128).saturating_mul(1_000_000_000_000_000));
        self.elapsed_ns =
            u64::try_from(self.elapsed_numerator / self.denominator).unwrap_or(u64::MAX);
        self.elapsed_ns
    }

    /// Changes clock rate without resetting the current timeline.
    pub fn set_drift_ppm(&mut self, drift_ppm: i32) -> Result<(), FakeEndpointError> {
        let mut next = Self::new(self.sample_rate_hz, drift_ppm)?;
        next.elapsed_ns = self.elapsed_ns;
        next.elapsed_numerator = u128::from(self.elapsed_ns) * next.denominator;
        *self = next;
        Ok(())
    }

    /// Returns the current ideal elapsed time.
    #[must_use]
    pub const fn elapsed_ns(&self) -> u64 {
        self.elapsed_ns
    }

    /// Returns the configured nominal rate and clock offset.
    #[must_use]
    pub const fn configuration(&self) -> (SampleRateHz, i32) {
        (self.sample_rate_hz, self.drift_ppm)
    }
}

/// One-based callback fault injected by a fake endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeEndpointFault {
    StreamError { at_callback: u64 },
    DeviceLost { at_callback: u64 },
}

impl FakeEndpointFault {
    const fn callback(self) -> u64 {
        match self {
            Self::StreamError { at_callback } | Self::DeviceLost { at_callback } => at_callback,
        }
    }
}

/// Timestamp and shape of one deterministic callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduledCallback {
    pub callback_index: u64,
    pub timestamp_ns: u64,
    pub frames: usize,
}

/// Callback observation from a fake source or sink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackEvent {
    Audio(ScheduledCallback),
    StreamError(ScheduledCallback),
    DeviceLost(ScheduledCallback),
}

impl CallbackEvent {
    /// Returns scheduling metadata for every event kind.
    #[must_use]
    pub const fn scheduled(self) -> ScheduledCallback {
        match self {
            Self::Audio(callback) | Self::StreamError(callback) | Self::DeviceLost(callback) => {
                callback
            }
        }
    }
}

/// Counters retained by a fake endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FakeEndpointStats {
    pub callbacks: u64,
    pub audio_frames: u64,
    pub stream_errors: u64,
    pub device_losses: u64,
    pub sample_sum: f64,
}

/// Deterministic interleaved source with independent clock, jitter, and faults.
#[derive(Debug)]
pub struct FakeAudioSource {
    channels: ChannelCount,
    sample_rate_hz: SampleRateHz,
    schedule: CallbackSchedule,
    clock: SimulatedClock,
    faults: Vec<FakeEndpointFault>,
    callback_index: u64,
    absolute_frame: u64,
    last_timestamp_ns: u64,
    disconnected: bool,
    stats: FakeEndpointStats,
}

impl FakeAudioSource {
    /// Creates a source. All allocations occur here.
    pub fn new(
        sample_rate_hz: SampleRateHz,
        channels: ChannelCount,
        drift_ppm: i32,
        schedule: CallbackSchedule,
        faults: Vec<FakeEndpointFault>,
    ) -> Result<Self, FakeEndpointError> {
        Ok(Self {
            channels,
            sample_rate_hz,
            schedule,
            clock: SimulatedClock::new(sample_rate_hz, drift_ppm)?,
            faults,
            callback_index: 0,
            absolute_frame: 0,
            last_timestamp_ns: 0,
            disconnected: false,
            stats: FakeEndpointStats::default(),
        })
    }

    pub fn set_drift_ppm(&mut self, ppm: i32) -> Result<(), FakeEndpointError> {
        self.clock.set_drift_ppm(ppm)
    }

    /// Returns the next jittered callback deadline, or `None` after device loss.
    #[must_use]
    pub fn next_deadline_ns(&self) -> Option<u64> {
        if self.disconnected {
            return None;
        }
        let (frames, jitter_ns) = self.schedule.peek();
        Some(jittered_timestamp(
            self.clock.deadline_after_frames(frames),
            jitter_ns,
            self.last_timestamp_ns,
        ))
    }

    /// Emits the next event and fills `output` for an audio callback.
    pub fn next_callback(
        &mut self,
        output: &mut [f32],
    ) -> Result<CallbackEvent, FakeEndpointError> {
        if self.disconnected {
            return Err(FakeEndpointError::Disconnected);
        }
        let (frames, jitter_ns) = self.schedule.peek();
        let required = frames
            .checked_mul(self.channels.as_usize())
            .ok_or(FakeEndpointError::BufferSizeOverflow)?;
        if output.len() < required {
            return Err(FakeEndpointError::CallbackBufferTooSmall {
                required,
                actual: output.len(),
            });
        }
        self.schedule.advance();
        self.callback_index += 1;
        let timestamp_ns = jittered_timestamp(
            self.clock.advance_frames(frames),
            jitter_ns,
            self.last_timestamp_ns,
        );
        self.last_timestamp_ns = timestamp_ns;
        let callback = ScheduledCallback {
            callback_index: self.callback_index,
            timestamp_ns,
            frames,
        };
        self.stats.callbacks += 1;

        let fault = self
            .faults
            .iter()
            .copied()
            .find(|fault| fault.callback() == self.callback_index);
        let first_frame = self.absolute_frame;
        self.absolute_frame = self.absolute_frame.saturating_add(frames as u64);
        match fault {
            Some(FakeEndpointFault::StreamError { .. }) => {
                self.stats.stream_errors += 1;
                Ok(CallbackEvent::StreamError(callback))
            }
            Some(FakeEndpointFault::DeviceLost { .. }) => {
                self.stats.device_losses += 1;
                self.disconnected = true;
                Ok(CallbackEvent::DeviceLost(callback))
            }
            None => {
                fill_source_samples(
                    &mut output[..required],
                    self.channels,
                    first_frame,
                    &mut self.stats.sample_sum,
                );
                self.stats.audio_frames += frames as u64;
                Ok(CallbackEvent::Audio(callback))
            }
        }
    }

    /// Returns the endpoint format.
    #[must_use]
    pub const fn format(&self) -> (SampleRateHz, ChannelCount) {
        (self.sample_rate_hz, self.channels)
    }

    /// Returns the largest callback in the configured cycle.
    #[must_use]
    pub fn max_callback_frames(&self) -> usize {
        self.schedule.max_frames()
    }

    /// Returns current endpoint counters.
    #[must_use]
    pub const fn stats(&self) -> FakeEndpointStats {
        self.stats
    }
}

/// Deterministic interleaved sink with bounded optional capture storage.
#[derive(Debug)]
pub struct FakeAudioSink {
    channels: ChannelCount,
    sample_rate_hz: SampleRateHz,
    schedule: CallbackSchedule,
    clock: SimulatedClock,
    faults: Vec<FakeEndpointFault>,
    callback_index: u64,
    last_timestamp_ns: u64,
    disconnected: bool,
    pending: Option<ScheduledCallback>,
    capture: Vec<f32>,
    captured_samples: usize,
    stats: FakeEndpointStats,
}

impl FakeAudioSink {
    /// Creates a sink and optionally preallocates capture storage for assertions.
    pub fn new(
        sample_rate_hz: SampleRateHz,
        channels: ChannelCount,
        drift_ppm: i32,
        schedule: CallbackSchedule,
        faults: Vec<FakeEndpointFault>,
        capture_capacity_frames: usize,
    ) -> Result<Self, FakeEndpointError> {
        let capture_samples = capture_capacity_frames
            .checked_mul(channels.as_usize())
            .ok_or(FakeEndpointError::BufferSizeOverflow)?;
        Ok(Self {
            channels,
            sample_rate_hz,
            schedule,
            clock: SimulatedClock::new(sample_rate_hz, drift_ppm)?,
            faults,
            callback_index: 0,
            last_timestamp_ns: 0,
            disconnected: false,
            pending: None,
            capture: vec![0.0; capture_samples],
            captured_samples: 0,
            stats: FakeEndpointStats::default(),
        })
    }

    /// Returns the next jittered callback deadline, or `None` after device loss.
    #[must_use]
    pub fn next_deadline_ns(&self) -> Option<u64> {
        if self.disconnected {
            return None;
        }
        let (frames, jitter_ns) = self.schedule.peek();
        Some(jittered_timestamp(
            self.clock.deadline_after_frames(frames),
            jitter_ns,
            self.last_timestamp_ns,
        ))
    }

    /// Requests the next output callback buffer. Audio events must be completed with [`Self::consume`].
    pub fn next_callback(&mut self) -> Result<CallbackEvent, FakeEndpointError> {
        if self.disconnected {
            return Err(FakeEndpointError::Disconnected);
        }
        if self.pending.is_some() {
            return Err(FakeEndpointError::PendingSinkCallback);
        }
        let (frames, jitter_ns) = self.schedule.advance();
        self.callback_index += 1;
        let timestamp_ns = jittered_timestamp(
            self.clock.advance_frames(frames),
            jitter_ns,
            self.last_timestamp_ns,
        );
        self.last_timestamp_ns = timestamp_ns;
        let callback = ScheduledCallback {
            callback_index: self.callback_index,
            timestamp_ns,
            frames,
        };
        self.stats.callbacks += 1;

        match self
            .faults
            .iter()
            .copied()
            .find(|fault| fault.callback() == self.callback_index)
        {
            Some(FakeEndpointFault::StreamError { .. }) => {
                self.stats.stream_errors += 1;
                Ok(CallbackEvent::StreamError(callback))
            }
            Some(FakeEndpointFault::DeviceLost { .. }) => {
                self.stats.device_losses += 1;
                self.disconnected = true;
                Ok(CallbackEvent::DeviceLost(callback))
            }
            None => {
                self.pending = Some(callback);
                Ok(CallbackEvent::Audio(callback))
            }
        }
    }

    /// Consumes exactly the samples requested by the pending audio callback.
    pub fn consume(&mut self, input: &[f32]) -> Result<(), FakeEndpointError> {
        let callback = self
            .pending
            .take()
            .ok_or(FakeEndpointError::NoPendingSinkCallback)?;
        let expected = callback
            .frames
            .checked_mul(self.channels.as_usize())
            .ok_or(FakeEndpointError::BufferSizeOverflow)?;
        if input.len() != expected {
            self.pending = Some(callback);
            return Err(FakeEndpointError::SinkBufferLength {
                expected,
                actual: input.len(),
            });
        }
        self.stats.audio_frames += callback.frames as u64;
        self.stats.sample_sum += input.iter().map(|sample| f64::from(*sample)).sum::<f64>();
        let capture_left = self.capture.len() - self.captured_samples;
        let copied = capture_left.min(input.len());
        self.capture[self.captured_samples..self.captured_samples + copied]
            .copy_from_slice(&input[..copied]);
        self.captured_samples += copied;
        Ok(())
    }

    /// Returns captured samples without unused preallocated storage.
    #[must_use]
    pub fn captured_samples(&self) -> &[f32] {
        &self.capture[..self.captured_samples]
    }

    /// Returns the endpoint format.
    #[must_use]
    pub const fn format(&self) -> (SampleRateHz, ChannelCount) {
        (self.sample_rate_hz, self.channels)
    }

    /// Returns the largest callback in the configured cycle.
    #[must_use]
    pub fn max_callback_frames(&self) -> usize {
        self.schedule.max_frames()
    }

    /// Returns current endpoint counters.
    #[must_use]
    pub const fn stats(&self) -> FakeEndpointStats {
        self.stats
    }
}

fn jittered_timestamp(ideal_ns: u64, jitter_ns: i64, previous_ns: u64) -> u64 {
    ideal_ns
        .saturating_add_signed(jitter_ns)
        .max(previous_ns.saturating_add(1))
}

fn fill_source_samples(
    output: &mut [f32],
    channels: ChannelCount,
    first_frame: u64,
    sample_sum: &mut f64,
) {
    let channels = channels.as_usize();
    for (frame_offset, frame) in output.chunks_exact_mut(channels).enumerate() {
        let index = first_frame.saturating_add(frame_offset as u64);
        let value = (index % 2_048) as f32 / 1_024.0 - 1.0;
        for (channel, sample) in frame.iter_mut().enumerate() {
            *sample = value + channel as f32 * 1.0e-5;
            *sample_sum += f64::from(*sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate() -> SampleRateHz {
        SampleRateHz::new(48_000).expect("valid test rate")
    }

    fn mono() -> ChannelCount {
        ChannelCount::new(1).expect("valid test channels")
    }

    #[test]
    fn positive_and_negative_drift_diverge_deterministically() {
        let mut fast = SimulatedClock::new(rate(), 100).expect("valid drift");
        let mut slow = SimulatedClock::new(rate(), -100).expect("valid drift");
        let fast_ns = fast.advance_frames(48_000 * 60);
        let slow_ns = slow.advance_frames(48_000 * 60);
        assert!(fast_ns < 60_000_000_000);
        assert!(slow_ns > 60_000_000_000);
        assert!((slow_ns - fast_ns).abs_diff(12_000_000) <= 1);
    }

    #[test]
    fn source_cycles_sizes_jitter_and_faults() {
        let schedule =
            CallbackSchedule::new(vec![64, 96], vec![-1_000, 2_000]).expect("valid schedule");
        let mut source = FakeAudioSource::new(
            rate(),
            mono(),
            100,
            schedule,
            vec![FakeEndpointFault::StreamError { at_callback: 2 }],
        )
        .expect("valid source");
        let mut buffer = [0.0; 96];
        assert!(matches!(
            source.next_callback(&mut buffer),
            Ok(CallbackEvent::Audio(ScheduledCallback { frames: 64, .. }))
        ));
        assert!(matches!(
            source.next_callback(&mut buffer),
            Ok(CallbackEvent::StreamError(ScheduledCallback {
                frames: 96,
                ..
            }))
        ));
        assert!(matches!(
            source.next_callback(&mut buffer),
            Ok(CallbackEvent::Audio(ScheduledCallback { frames: 64, .. }))
        ));
        assert_eq!(source.stats().audio_frames, 128);
        assert_eq!(source.stats().stream_errors, 1);
    }

    #[test]
    fn sink_capture_is_preallocated_and_device_loss_is_terminal() {
        let schedule = CallbackSchedule::without_jitter(vec![4]).expect("valid schedule");
        let mut sink = FakeAudioSink::new(
            rate(),
            mono(),
            0,
            schedule,
            vec![FakeEndpointFault::DeviceLost { at_callback: 2 }],
            4,
        )
        .expect("valid sink");
        let callback = sink.next_callback().expect("first callback");
        assert!(matches!(callback, CallbackEvent::Audio(_)));
        sink.consume(&[1.0, 2.0, 3.0, 4.0])
            .expect("matching callback buffer");
        assert_eq!(sink.captured_samples(), [1.0, 2.0, 3.0, 4.0]);
        assert!(matches!(
            sink.next_callback(),
            Ok(CallbackEvent::DeviceLost(_))
        ));
        assert_eq!(sink.next_callback(), Err(FakeEndpointError::Disconnected));
    }
}
