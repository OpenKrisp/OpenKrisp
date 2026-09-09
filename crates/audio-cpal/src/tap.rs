//! Native-only callback observations. No type in this module implements serialization.
use crate::{AudioIoError, StreamDirection};
use audio_protocol::AudioFormat;
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Observer, Producer, Split},
};
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

pub const TAP_BLOCK_FRAMES: usize = 256;
pub const TAP_CAPACITY_BLOCKS: usize = 64;
pub const TAP_MAX_AGE_NS: u64 = 200_000_000;
const MAX_DEVICE_DELAY_NS: i128 = 2_000_000_000;
static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);
static CLOCK_ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Process-local monotonic nanoseconds; never wall time. Initialize outside callbacks.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock {
    origin: Instant,
}
impl Default for MonotonicClock {
    fn default() -> Self {
        Self {
            origin: *CLOCK_ORIGIN.get_or_init(Instant::now),
        }
    }
}
impl MonotonicClock {
    pub fn now_ns(self) -> u64 {
        self.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }
}

/// First frame of a block. Device time is comparable only within `stream_id` and `epoch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioTimestamp {
    pub stream_id: u64,
    pub epoch: u64,
    pub frame_position: u64,
    pub callback_frame_position: u64,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub direction: StreamDirection,
    /// Arrival of the original CPAL callback on the process monotonic clock.
    pub observed_ns: u64,
    /// CPAL's opaque per-stream time, adjusted for the chunk's first frame.
    pub device_time_ns: Option<u64>,
    /// Arrival + (CPAL capture/playback - CPAL callback) + chunk offset.
    /// Signed because capture can precede process clock initialization. Estimated, never measured.
    pub estimated_time_ns: Option<i64>,
}
impl AudioTimestamp {
    pub(crate) fn offset(mut self, frames: usize) -> Self {
        let offset = self
            .frame_position
            .saturating_sub(self.callback_frame_position);
        let rate = u128::from(self.sample_rate_hz);
        let ns = ((u128::from(offset) + frames as u128) * 1_000_000_000 / rate
            - u128::from(offset) * 1_000_000_000 / rate) as u64;
        self.frame_position = self.frame_position.saturating_add(frames as u64);
        self.device_time_ns = self.device_time_ns.and_then(|t| t.checked_add(ns));
        self.estimated_time_ns = self
            .estimated_time_ns
            .and_then(|t| t.checked_add(ns as i64));
        self
    }
}

/// Interleaved device-channel order, normalized f32 reconstructed from submitted device PCM.
/// Check `AudioTapReader::is_current` again before using a retained block.
#[derive(Clone, Debug)]
pub struct TimedAudioBlock {
    pub timestamp: AudioTimestamp,
    pub frames: usize,
    pub discontinuity: bool,
    samples: [f32; TAP_BLOCK_FRAMES * 2],
}
impl TimedAudioBlock {
    pub fn samples(&self) -> &[f32] {
        &self.samples[..self.frames * usize::from(self.timestamp.channels)]
    }
}

#[derive(Debug)]
struct TapState {
    enabled: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    closed: AtomicBool,
    epoch: AtomicU64,
    overflow_events: AtomicU64,
    queued_frames: AtomicUsize,
}
/// An owner/error-callback invalidation handle. Closing is permanent for this stream generation.
#[derive(Clone, Debug)]
pub struct AudioTapControl {
    state: Arc<TapState>,
}
impl AudioTapControl {
    pub fn close(&self) {
        self.state.closed.store(true, Ordering::Release);
    }
    pub fn discontinue(&self) {
        self.state.epoch.fetch_add(1, Ordering::AcqRel);
    }
    pub fn is_closed(&self) -> bool {
        self.state.closed.load(Ordering::Acquire) || self.state.cancelled.load(Ordering::Acquire)
    }
    pub fn is_active(&self) -> bool {
        !self.state.closed.load(Ordering::Acquire)
            && !self.state.cancelled.load(Ordering::Acquire)
            && self.state.enabled.load(Ordering::Acquire)
    }
    pub fn epoch(&self) -> u64 {
        self.state.epoch.load(Ordering::Acquire)
    }
    pub fn overflow_events(&self) -> u64 {
        self.state.overflow_events.load(Ordering::Relaxed)
    }
}

pub struct AudioTapWriter {
    inner: HeapProd<TimedAudioBlock>,
    control: AudioTapControl,
    stream_id: u64,
    format: AudioFormat,
    direction: StreamDirection,
    clock: MonotonicClock,
    frame_position: u64,
    previous_callback: Option<(u64, u64, u64, usize)>,
    max_frames: usize,
}
impl AudioTapWriter {
    pub(crate) fn validate_format(
        &self,
        format: &AudioFormat,
        direction: StreamDirection,
    ) -> Result<(), AudioIoError> {
        if self.format.sample_rate_hz != format.sample_rate_hz
            || self.format.channels != format.channels
            || self.format.sample_format != format.sample_format
            || self.direction != direction
        {
            return Err(AudioIoError::InvalidAudioTapFormat);
        }
        Ok(())
    }
    pub fn control(&self) -> AudioTapControl {
        self.control.clone()
    }
    pub(crate) fn clock(&self) -> MonotonicClock {
        self.clock
    }
    /// One timestamp observation per native callback, before splitting into conversion chunks.
    pub(crate) fn timestamp(
        &mut self,
        callback_ns: u128,
        audio_ns: u128,
        observed_ns: u64,
        frames: usize,
    ) -> AudioTimestamp {
        let callback = u64::try_from(callback_ns).ok();
        let audio = u64::try_from(audio_ns).ok();
        let delta = audio_ns as i128 - callback_ns as i128;
        let valid_direction = match self.direction {
            StreamDirection::Input => delta <= 0,
            StreamDirection::Output => delta >= 0,
        };
        let mut valid = callback.is_some()
            && audio.is_some()
            && audio_ns != 0
            && callback_ns != 0
            && delta.abs() <= MAX_DEVICE_DELAY_NS
            && valid_direction;
        if let (Some(cb), Some(at), Some((last_cb, last_at, last_observed, last_frames))) =
            (callback, audio, self.previous_callback)
        {
            let expected_ns = (last_frames as u128 * 1_000_000_000
                / u128::from(self.format.sample_rate_hz.get()))
                as u64;
            // A gap in the device-frame timeline must not be fitted as device clock drift.
            valid &= cb > last_cb
                && at > last_at
                && at.saturating_sub(last_at).abs_diff(expected_ns) <= 20_000_000
                && cb
                    .saturating_sub(last_cb)
                    .abs_diff(observed_ns.saturating_sub(last_observed))
                    <= 100_000_000;
        }
        if !valid {
            self.control.discontinue();
        }
        self.previous_callback = callback
            .zip(audio)
            .map(|(cb, at)| (cb, at, observed_ns, frames));
        let timestamp = AudioTimestamp {
            stream_id: self.stream_id,
            epoch: self.control.state.epoch.load(Ordering::Acquire),
            frame_position: self.frame_position,
            callback_frame_position: self.frame_position,
            sample_rate_hz: self.format.sample_rate_hz.get(),
            channels: self.format.channels.get(),
            direction: self.direction,
            observed_ns,
            device_time_ns: valid.then_some(audio).flatten(),
            estimated_time_ns: valid
                .then(|| i64::try_from(i128::from(observed_ns) + delta).ok())
                .flatten(),
        };
        self.frame_position = self.frame_position.saturating_add(frames as u64);
        timestamp
    }
    /// Bounded copy; a full tap never changes the primary audio route.
    pub(crate) fn write(&mut self, samples: &[f32], timestamp: AudioTimestamp) {
        if !self.control.is_active()
            || timestamp.epoch != self.control.state.epoch.load(Ordering::Acquire)
        {
            return;
        }
        let channels = self.format.channels.as_usize();
        for (index, chunk) in samples.chunks(TAP_BLOCK_FRAMES * channels).enumerate() {
            let frames = chunk.len() / channels;
            if self.inner.is_full()
                || self.control.state.queued_frames.load(Ordering::Acquire) + frames
                    > self.max_frames
            {
                self.control
                    .state
                    .overflow_events
                    .fetch_add(1, Ordering::Relaxed);
                self.control.discontinue();
                return;
            }
            let mut block = TimedAudioBlock {
                timestamp: timestamp.offset(index * TAP_BLOCK_FRAMES),
                frames,
                discontinuity: false,
                samples: [0.0; TAP_BLOCK_FRAMES * 2],
            };
            block.samples[..chunk.len()].copy_from_slice(chunk);
            // Publish accounting before the SPSC write so the consumer cannot subtract first.
            self.control
                .state
                .queued_frames
                .fetch_add(frames, Ordering::AcqRel);
            if self.inner.try_push(block).is_err() {
                self.control
                    .state
                    .queued_frames
                    .fetch_sub(frames, Ordering::AcqRel);
                self.control
                    .state
                    .overflow_events
                    .fetch_add(1, Ordering::Relaxed);
                self.control.discontinue();
            }
        }
    }
}
impl Drop for AudioTapWriter {
    fn drop(&mut self) {
        self.control.close();
    }
}

pub struct AudioTapReader {
    inner: HeapCons<TimedAudioBlock>,
    control: AudioTapControl,
    stream_id: u64,
    clock: MonotonicClock,
    previous: Option<(u64, u64)>,
}
impl AudioTapReader {
    pub fn control(&self) -> AudioTapControl {
        self.control.clone()
    }
    pub fn is_active(&self) -> bool {
        self.control.is_active()
    }
    pub fn is_closed(&self) -> bool {
        self.control.is_closed()
    }
    pub fn is_current(&self, block: &TimedAudioBlock) -> bool {
        self.is_current_at(block, self.clock.now_ns())
    }
    fn is_current_at(&self, block: &TimedAudioBlock, now_ns: u64) -> bool {
        self.is_active()
            && block.timestamp.stream_id == self.stream_id
            && block.timestamp.epoch == self.control.state.epoch.load(Ordering::Acquire)
            && now_ns.saturating_sub(block.timestamp.observed_ns) <= TAP_MAX_AGE_NS
    }
    /// At most 64 fixed-size pops. Returns no queued audio after Stop, close or expiry.
    pub fn read(&mut self) -> Option<TimedAudioBlock> {
        self.read_at(self.clock.now_ns())
    }
    fn read_at(&mut self, now_ns: u64) -> Option<TimedAudioBlock> {
        for _ in 0..TAP_CAPACITY_BLOCKS {
            let mut block = self.inner.try_pop()?;
            self.control
                .state
                .queued_frames
                .fetch_sub(block.frames, Ordering::AcqRel);
            if !self.is_current_at(&block, now_ns) {
                self.previous = None;
                continue;
            }
            block.discontinuity =
                self.previous != Some((block.timestamp.epoch, block.timestamp.frame_position));
            self.previous = Some((
                block.timestamp.epoch,
                block
                    .timestamp
                    .frame_position
                    .saturating_add(block.frames as u64),
            ));
            return Some(block);
        }
        None
    }
}

/// Fixed 64-block / 200-ms frame-budget tap. Mono/stereo only. Allocate on the route owner.
pub fn audio_tap(
    format: AudioFormat,
    direction: StreamDirection,
    enabled: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
) -> Result<(AudioTapWriter, AudioTapReader), AudioIoError> {
    if !(1..=2).contains(&format.channels.get()) {
        return Err(AudioIoError::InvalidAudioTapFormat);
    }
    let max_frames = (u64::from(format.sample_rate_hz.get()) / 5).max(1) as usize;
    let (inner_tx, inner_rx) = HeapRb::new(TAP_CAPACITY_BLOCKS).split();
    let stream_id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
    let control = AudioTapControl {
        state: Arc::new(TapState {
            enabled,
            cancelled,
            closed: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
            overflow_events: AtomicU64::new(0),
            queued_frames: AtomicUsize::new(0),
        }),
    };
    let clock = MonotonicClock::default();
    Ok((
        AudioTapWriter {
            inner: inner_tx,
            control: control.clone(),
            stream_id,
            format,
            direction,
            clock,
            frame_position: 0,
            previous_callback: None,
            max_frames,
        },
        AudioTapReader {
            inner: inner_rx,
            control,
            stream_id,
            clock,
            previous: None,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_protocol::{ChannelCount, SampleFormat, SampleRateHz};
    fn make(
        rate: u32,
        channels: u16,
        direction: StreamDirection,
    ) -> (AudioTapWriter, AudioTapReader) {
        audio_tap(
            AudioFormat {
                sample_rate_hz: SampleRateHz::new(rate).unwrap(),
                channels: ChannelCount::new(channels).unwrap(),
                sample_format: SampleFormat::F32,
                buffer_size_frames: None,
            },
            direction,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    }
    fn stamp(writer: &mut AudioTapWriter, frames: usize, time: u64) -> AudioTimestamp {
        writer.timestamp(
            u128::from(time),
            u128::from(time + 10_000_000),
            time,
            frames,
        )
    }
    #[test]
    fn stereo_reference_preserves_channel_order_and_fractional_chunk_offsets() {
        let (mut tx, mut rx) = make(44_100, 2, StreamDirection::Output);
        let samples: Vec<f32> = (0..1200)
            .map(|n| if n % 2 == 0 { n as f32 } else { -(n as f32) })
            .collect();
        let ts = stamp(&mut tx, 600, 1_000_000_000);
        tx.write(&samples, ts);
        let mut actual = Vec::new();
        for (index, frames) in [256, 256, 88].into_iter().enumerate() {
            let block = rx.read_at(1_000_000_000).unwrap();
            assert_eq!(block.frames, frames);
            assert_eq!(block.discontinuity, index == 0);
            assert_eq!(block.timestamp.frame_position, (index * 256) as u64);
            assert_eq!(block.timestamp.callback_frame_position, 0);
            assert_eq!(
                block.timestamp.estimated_time_ns,
                Some(1_010_000_000 + (index * 256 * 1_000_000_000 / 44_100) as i64)
            );
            actual.extend_from_slice(block.samples());
        }
        assert_eq!(actual, samples);
        assert!(rx.read_at(1_000_000_000).is_none());
    }
    #[test]
    fn a_device_timeline_gap_invalidates_old_audio_and_recovers_on_the_next_callback() {
        let (mut tx, mut rx) = make(48_000, 1, StreamDirection::Output);
        let first = stamp(&mut tx, 480, 1_000_000_000);
        tx.write(&[0.1; 480], first);
        let retained = rx.read_at(first.observed_ns).unwrap();
        // Both wall time and the device clock advance normally through a one-second stall;
        // only the comparison with the previous buffer's frame count reveals the missing audio.
        let gap = stamp(&mut tx, 480, 2_010_000_000);
        tx.write(&[0.2; 480], gap);
        assert!(gap.estimated_time_ns.is_none());
        assert!(gap.epoch > first.epoch);
        assert!(!rx.is_current_at(&retained, first.observed_ns));
        while rx.read_at(gap.observed_ns).is_some() {}
        let next = stamp(&mut tx, 480, 2_020_000_000);
        tx.write(&[0.3; 480], next);
        assert_eq!(next.estimated_time_ns, Some(2_030_000_000));
        assert_eq!(next.epoch, gap.epoch);
        assert_eq!(rx.read_at(next.observed_ns).unwrap().samples(), &[0.3; 256]);
    }

    #[test]
    fn tap_format_validation_rejects_channel_rate_sample_type_and_direction_mismatches() {
        let (tx, _rx) = make(48_000, 2, StreamDirection::Output);
        assert!(
            tx.validate_format(&tx.format, StreamDirection::Output)
                .is_ok()
        );
        assert!(
            tx.validate_format(&tx.format, StreamDirection::Input)
                .is_err()
        );
        let mut other = tx.format.clone();
        other.channels = ChannelCount::new(1).unwrap();
        assert!(tx.validate_format(&other, StreamDirection::Output).is_err());
        other = tx.format.clone();
        other.sample_rate_hz = SampleRateHz::new(44_100).unwrap();
        assert!(tx.validate_format(&other, StreamDirection::Output).is_err());
        other = tx.format.clone();
        other.sample_format = SampleFormat::I16;
        assert!(tx.validate_format(&other, StreamDirection::Output).is_err());
        other.channels = ChannelCount::new(3).unwrap();
        assert!(
            audio_tap(
                other,
                StreamDirection::Output,
                Arc::new(AtomicBool::new(true)),
                Arc::new(AtomicBool::new(false))
            )
            .is_err()
        );
    }

    #[test]
    fn independent_native_origins_map_into_the_same_process_clock() {
        let (mut capture, _) = make(48_000, 1, StreamDirection::Input);
        let (mut render, _) = make(48_000, 2, StreamDirection::Output);
        let c = capture.timestamp(50_020_000_000, 50_000_000_000, 1_020_000_000, 480);
        let r = render.timestamp(80_990_000_000, 81_000_000_000, 990_000_000, 480);
        assert_ne!(c.device_time_ns, r.device_time_ns);
        assert_ne!(c.stream_id, r.stream_id);
        assert_eq!(c.estimated_time_ns, Some(1_000_000_000));
        assert_eq!(c.estimated_time_ns, r.estimated_time_ns);
    }
    #[test]
    fn invalid_or_regressing_timestamps_clear_the_epoch_and_do_not_invent_time() {
        let (mut tx, mut rx) = make(48_000, 1, StreamDirection::Output);
        let initial = stamp(&mut tx, 1, 1_000_000_000);
        tx.write(&[0.4], initial);
        for (cb, audio) in [
            (900_000_000, 910_000_000),
            (0, 0),
            (10, 9),
            (10, 3_000_000_010),
        ] {
            let bad = tx.timestamp(cb, audio, 1_010_000_000, 1);
            assert!(bad.estimated_time_ns.is_none());
            assert!(bad.device_time_ns.is_none());
            assert!(bad.epoch > initial.epoch);
        }
        assert!(rx.read_at(1_010_000_000).is_none());
    }
    #[test]
    fn overflow_invalidates_queued_and_retained_audio_then_recovers_with_a_gap() {
        let (mut tx, mut rx) = make(48_000, 2, StreamDirection::Output);
        let t = stamp(&mut tx, 1, 1_000_000_000);
        tx.write(&[0.1, 0.2], t);
        let retained = rx.read_at(1_000_000_000).unwrap();
        for i in 0..TAP_CAPACITY_BLOCKS + 1 {
            let t = stamp(&mut tx, 1, 1_000_000_001 + i as u64);
            tx.write(&[0.1, 0.2], t);
        }
        assert_eq!(tx.control.overflow_events(), 1);
        assert!(tx.control.state.queued_frames.load(Ordering::Acquire) <= TAP_CAPACITY_BLOCKS);
        assert!(!rx.is_current_at(&retained, 1_000_000_100));
        assert!(rx.read_at(1_000_000_100).is_none());
        let t = stamp(&mut tx, 1, 1_000_000_100);
        tx.write(&[0.3, 0.4], t);
        let recovered = rx.read_at(1_000_000_100).unwrap();
        assert!(recovered.discontinuity);
        assert_eq!(recovered.samples(), &[0.3, 0.4]);
    }
    #[test]
    fn frame_budget_and_wall_age_are_both_bounded() {
        for rate in [8_000, 44_100, 48_000, 96_000, 192_000] {
            let (mut tx, mut rx) = make(rate, 1, StreamDirection::Output);
            for i in 0..100 {
                let t = stamp(&mut tx, 256, 1_000_000_000 + i);
                tx.write(&[0.1; 256], t);
                assert!(
                    tx.control.state.queued_frames.load(Ordering::Acquire) <= rate as usize / 5
                );
                assert!(tx.inner.occupied_len() <= TAP_CAPACITY_BLOCKS);
            }
            assert!(tx.control.overflow_events() > 0);
            assert!(rx.read_at(1_000_000_100 + TAP_MAX_AGE_NS + 1).is_none());
            assert_eq!(tx.control.state.queued_frames.load(Ordering::Acquire), 0);
        }
    }
    #[test]
    fn stop_drop_and_new_stream_never_replay_old_reference() {
        let (mut tx, mut rx) = make(48_000, 1, StreamDirection::Output);
        let t = stamp(&mut tx, 2, 1_000_000_000);
        tx.write(&[0.1, 0.2], t);
        tx.control.state.cancelled.store(true, Ordering::Release);
        assert!(rx.is_closed());
        assert!(rx.read_at(1_000_000_000).is_none());
        let (mut next, mut next_rx) = make(48_000, 1, StreamDirection::Output);
        let next_t = stamp(&mut next, 1, 1_000_000_000);
        assert_ne!(next_t.stream_id, t.stream_id);
        assert_eq!(next_t.frame_position, 0);
        next.write(&[0.3], next_t);
        let block = next_rx.read_at(1_000_000_000).unwrap();
        drop(next);
        assert!(next_rx.is_closed());
        assert!(!next_rx.is_current_at(&block, 1_000_000_000));
        assert!(!rx.is_current_at(&block, 1_000_000_000));
    }
    #[test]
    fn startup_gate_and_error_invalidation_work_without_another_audio_callback() {
        let (mut tx, mut rx) = make(48_000, 1, StreamDirection::Output);
        tx.control.state.enabled.store(false, Ordering::Release);
        let t = stamp(&mut tx, 1, 1_000_000_000);
        tx.write(&[0.2], t);
        assert!(rx.read_at(1_000_000_000).is_none());
        tx.control.state.enabled.store(true, Ordering::Release);
        let t = stamp(&mut tx, 1, 1_001_000_000);
        tx.write(&[0.3], t);
        let block = rx.read_at(1_001_000_000).unwrap();
        let error_control = tx.control();
        error_control.discontinue();
        assert!(!rx.is_current_at(&block, 1_001_000_000));
        error_control.close();
        assert!(rx.is_closed());
        assert!(rx.read_at(1_001_000_000).is_none());
    }
    #[test]
    fn tap_steady_state_overflow_and_invalidation_allocate_nothing() {
        let (mut tx, mut rx) = make(48_000, 2, StreamDirection::Output);
        let allocations = allocation_counter::measure(|| {
            for i in 0..500 {
                let t = stamp(&mut tx, 256, 1_000_000_000 + i * 5_333_333);
                tx.write(&[0.1; 512], t);
                if i % 100 == 0 {
                    tx.control.discontinue();
                }
                if i % 2 == 0 {
                    let _ = rx.read_at(t.observed_ns);
                }
            }
            tx.control.close();
            let _ = rx.read_at(4_000_000_000);
        });
        assert_eq!(allocations.count_total, 0, "{allocations:?}");
    }
}
