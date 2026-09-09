//! Consumer-side timing estimates for a future AEC. These do not alter the drift controller.
use audio_cpal::{
    AudioTapControl, AudioTapReader, AudioTimestamp, StreamDirection, TAP_MAX_AGE_NS,
    TimedAudioBlock,
};
const CLOCK_POINTS: usize = 128;
const MAX_RATE_ERROR: f64 = 0.005;

/// Bounded least-squares mapping from one stream's frame counter to process monotonic time.
/// Scheduling jitter and backend timestamp approximations remain in this estimate.
#[derive(Clone, Debug)]
pub struct AudioClockTracker {
    points: [Option<AudioTimestamp>; CLOCK_POINTS],
    next: usize,
    latest: Option<AudioTimestamp>,
    base_frame: u64,
    base_time_ns: f64,
    ns_per_frame: f64,
    rate_ready: bool,
}
impl Default for AudioClockTracker {
    fn default() -> Self {
        Self {
            points: [None; CLOCK_POINTS],
            next: 0,
            latest: None,
            base_frame: 0,
            base_time_ns: 0.0,
            ns_per_frame: 0.0,
            rate_ready: false,
        }
    }
}
impl AudioClockTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Feed blocks in reader order. Unknown timestamps and gaps clear all history.
    pub fn observe(&mut self, block: &TimedAudioBlock) {
        self.observe_timestamp(block.timestamp, block.discontinuity);
    }
    /// Metadata-only clock fitting. The caller must reset on reader invalidation and check freshness.
    pub fn observe_timestamp(&mut self, t: AudioTimestamp, discontinuity: bool) {
        if t.sample_rate_hz == 0 {
            self.reset();
            return;
        }
        if discontinuity
            || self.latest.is_some_and(|last| {
                last.stream_id != t.stream_id
                    || last.epoch != t.epoch
                    || last.sample_rate_hz != t.sample_rate_hz
                    || last.direction != t.direction
                    || t.frame_position <= last.frame_position
                    || t.estimated_time_ns <= last.estimated_time_ns
            })
        {
            self.reset();
        }
        if t.estimated_time_ns.is_none() || t.device_time_ns.is_none() {
            self.reset();
            return;
        }
        self.points[self.next] = Some(t);
        self.next = (self.next + 1) % CLOCK_POINTS;
        self.latest = Some(t);
        self.fit();
    }
    fn fit(&mut self) {
        let Some(latest) = self.latest else {
            return;
        };
        let Some(latest_ns) = latest.estimated_time_ns else {
            return;
        };
        let nominal = 1_000_000_000.0 / f64::from(latest.sample_rate_hz);
        let mut n = 0.0;
        let (mut x_sum, mut y_sum, mut xx, mut xy) = (0.0, 0.0, 0.0, 0.0);
        let mut span_frames = 0;
        for t in self.points.iter().flatten() {
            let x = (i128::from(t.frame_position) - i128::from(latest.frame_position)) as f64;
            let y = (i128::from(t.estimated_time_ns.unwrap_or(latest_ns)) - i128::from(latest_ns))
                as f64;
            n += 1.0;
            x_sum += x;
            y_sum += y;
            xx += x * x;
            xy += x * y;
            span_frames = span_frames.max(latest.frame_position.saturating_sub(t.frame_position));
        }
        let variance = xx - x_sum * x_sum / n;
        let fitted = if variance > 0.0 {
            (xy - x_sum * y_sum / n) / variance
        } else {
            nominal
        };
        self.rate_ready = span_frames >= u64::from(latest.sample_rate_hz) / 10
            && (fitted / nominal - 1.0).abs() <= MAX_RATE_ERROR;
        self.ns_per_frame = if self.rate_ready { fitted } else { nominal };
        self.base_frame = latest.frame_position;
        self.base_time_ns = latest_ns as f64 + (y_sum - self.ns_per_frame * x_sum) / n;
    }
    fn fresh(&self, now_ns: u64) -> bool {
        self.latest
            .is_some_and(|t| now_ns.saturating_sub(t.observed_ns) <= TAP_MAX_AGE_NS)
    }
    /// Estimates a frame's time within the retained window plus at most 200 ms extrapolation.
    pub fn time_at_frame(&self, frame: u64, now_ns: u64) -> Option<i64> {
        if !self.fresh(now_ns) {
            return None;
        }
        let latest = self.latest?;
        let oldest = self
            .points
            .iter()
            .flatten()
            .map(|t| t.frame_position)
            .min()?;
        let margin = u64::from(latest.sample_rate_hz) / 5;
        if frame < oldest.saturating_sub(margin)
            || frame > latest.frame_position.saturating_add(margin)
        {
            return None;
        }
        let delta = (i128::from(frame) - i128::from(self.base_frame)) as f64;
        Some(self.ns_per_frame.mul_add(delta, self.base_time_ns).round() as i64)
    }
    pub fn frame_at_time(&self, time_ns: i64, now_ns: u64) -> Option<f64> {
        if !self.fresh(now_ns) || self.ns_per_frame <= 0.0 {
            return None;
        }
        let frame =
            self.base_frame as f64 + (time_ns as f64 - self.base_time_ns) / self.ns_per_frame;
        if !frame.is_finite() || frame < 0.0 {
            return None;
        }
        self.time_at_frame(frame.round() as u64, now_ns)?;
        Some(frame)
    }
    pub fn rate_error_ppm(&self) -> Option<f64> {
        let t = self.latest?;
        self.rate_ready.then(|| {
            (1_000_000_000.0 / (self.ns_per_frame * f64::from(t.sample_rate_hz)) - 1.0)
                * 1_000_000.0
        })
    }
}

/// CPAL capture age plus predicted render lead, in nanoseconds. Excludes DSP queues, model
/// latency, acoustic propagation and unreported transport latency. Never an end-to-end measure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DuplexDelayEstimate {
    pub capture_age_ns: i64,
    pub render_lead_ns: i64,
    pub io_delay_ns: i64,
    pub smoothed_io_delay_ns: i64,
    pub relative_rate_ppm: Option<f64>,
}
#[derive(Default)]
pub struct DuplexDelayTracker {
    capture: AudioClockTracker,
    render: AudioClockTracker,
    smoothed: Option<f64>,
    last_pair: Option<(u64, u64)>,
    capture_control: Option<AudioTapControl>,
    render_control: Option<AudioTapControl>,
}
impl DuplexDelayTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn observe(&mut self, reader: &AudioTapReader, block: &TimedAudioBlock) {
        if !reader.is_current(block) {
            self.reset();
            return;
        }
        match block.timestamp.direction {
            StreamDirection::Input => self.capture_control = Some(reader.control()),
            StreamDirection::Output => self.render_control = Some(reader.control()),
        }
        self.observe_timestamp(block.timestamp, block.discontinuity);
    }
    fn observe_timestamp(&mut self, timestamp: AudioTimestamp, discontinuity: bool) {
        let target = match timestamp.direction {
            StreamDirection::Input => &mut self.capture,
            StreamDirection::Output => &mut self.render,
        };
        let old = target.latest;
        target.observe_timestamp(timestamp, discontinuity);
        if discontinuity
            || target.latest.is_none()
            || old.is_some_and(|t| t.stream_id != timestamp.stream_id || t.epoch != timestamp.epoch)
        {
            self.smoothed = None;
            self.last_pair = None;
        }
    }
    /// Find the fractional render frame contemporaneous with a microphone frame. Resampling and
    /// acoustic delay correction belong to the future AEC consumer, not this timing utility.
    pub fn render_frame_for_capture(&self, capture_frame: u64, now_ns: u64) -> Option<f64> {
        if !self.controls_current() {
            return None;
        }
        let time_ns = self.capture.time_at_frame(capture_frame, now_ns)?;
        self.render.frame_at_time(time_ns, now_ns)
    }
    fn controls_current(&self) -> bool {
        [
            (&self.capture_control, &self.capture),
            (&self.render_control, &self.render),
        ]
        .into_iter()
        .all(|(control, clock)| {
            control.as_ref().is_some_and(|control| {
                control.is_active() && clock.latest.is_some_and(|t| t.epoch == control.epoch())
            })
        })
    }
    pub fn estimate(&mut self, now_ns: u64) -> Option<DuplexDelayEstimate> {
        if !self.controls_current() || !self.capture.fresh(now_ns) || !self.render.fresh(now_ns) {
            self.smoothed = None;
            self.last_pair = None;
            return None;
        }
        let c = self.capture.latest?;
        let r = self.render.latest?;
        // Remove chunk offsets; callback arrival belongs to the first native buffer frame.
        let callback_time = |t: AudioTimestamp| -> Option<i64> {
            let offset = t.frame_position.saturating_sub(t.callback_frame_position);
            let ns = u128::from(offset) * 1_000_000_000 / u128::from(t.sample_rate_hz);
            t.estimated_time_ns?.checked_sub(i64::try_from(ns).ok()?)
        };
        let capture_age_ns = i64::try_from(c.observed_ns)
            .ok()?
            .checked_sub(callback_time(c)?)?;
        let render_lead_ns = callback_time(r)?.checked_sub(i64::try_from(r.observed_ns).ok()?)?;
        let io_delay_ns = capture_age_ns.checked_add(render_lead_ns)?;
        let pair = (c.callback_frame_position, r.callback_frame_position);
        if self.last_pair != Some(pair) {
            self.smoothed = Some(self.smoothed.map_or(io_delay_ns as f64, |last| {
                last + 0.1 * (io_delay_ns as f64 - last)
            }));
            self.last_pair = Some(pair);
        }
        let relative_rate_ppm = self
            .capture
            .rate_error_ppm()
            .zip(self.render.rate_error_ppm())
            .map(|(c, r)| ((1.0 + c / 1_000_000.0) / (1.0 + r / 1_000_000.0) - 1.0) * 1_000_000.0);
        Some(DuplexDelayEstimate {
            capture_age_ns,
            render_lead_ns,
            io_delay_ns,
            smoothed_io_delay_ns: self.smoothed?.round() as i64,
            relative_rate_ppm,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_cpal::audio_tap;
    use audio_protocol::{AudioFormat, ChannelCount, SampleFormat, SampleRateHz};
    use std::sync::{Arc, atomic::AtomicBool};
    fn timestamp(
        direction: StreamDirection,
        frame: u64,
        rate: u32,
        drift_ppm: f64,
        delay_ns: i64,
    ) -> AudioTimestamp {
        let elapsed = (frame as f64 * 1_000_000_000.0
            / (f64::from(rate) * (1.0 + drift_ppm / 1_000_000.0)))
            .round() as i64;
        let audio_ns = 1_000_000_000 + elapsed;
        let observed_ns = match direction {
            StreamDirection::Input => audio_ns + delay_ns,
            StreamDirection::Output => audio_ns - delay_ns,
        } as u64;
        AudioTimestamp {
            stream_id: if direction == StreamDirection::Input {
                1
            } else {
                2
            },
            epoch: 0,
            frame_position: frame,
            callback_frame_position: frame,
            sample_rate_hz: rate,
            channels: 1,
            direction,
            observed_ns,
            device_time_ns: Some(
                (audio_ns
                    + if direction == StreamDirection::Input {
                        50_000_000_000
                    } else {
                        80_000_000_000
                    }) as u64,
            ),
            estimated_time_ns: Some(audio_ns),
        }
    }
    fn controlled_tracker() -> (
        DuplexDelayTracker,
        audio_cpal::AudioTapWriter,
        audio_cpal::AudioTapWriter,
    ) {
        let make = |direction| {
            audio_tap(
                AudioFormat {
                    sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
                    channels: ChannelCount::new(1).unwrap(),
                    sample_format: SampleFormat::F32,
                    buffer_size_frames: None,
                },
                direction,
                Arc::new(AtomicBool::new(true)),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap()
            .0
        };
        let capture = make(StreamDirection::Input);
        let render = make(StreamDirection::Output);
        let tracker = DuplexDelayTracker {
            capture_control: Some(capture.control()),
            render_control: Some(render.control()),
            ..DuplexDelayTracker::default()
        };
        (tracker, capture, render)
    }
    #[test]
    fn independent_rates_drifts_and_origins_align_a_microphone_frame_with_render() {
        for drift in [-750.0, -100.0, 100.0, 750.0] {
            let (mut tracker, _capture, _render) = controlled_tracker();
            for i in 0..400 {
                tracker.observe_timestamp(
                    timestamp(StreamDirection::Input, i * 240, 48_000, drift, 20_000_000),
                    i == 0,
                );
                tracker.observe_timestamp(
                    timestamp(StreamDirection::Output, i * 220, 44_100, -drift, 10_000_000),
                    i == 0,
                );
            }
            let now = tracker
                .capture
                .latest
                .unwrap()
                .observed_ns
                .max(tracker.render.latest.unwrap().observed_ns);
            let c_frame = 399 * 240;
            let estimated = tracker.render_frame_for_capture(c_frame, now).unwrap();
            let expected = c_frame as f64 / 48_000.0 / (1.0 + drift / 1_000_000.0)
                * 44_100.0
                * (1.0 - drift / 1_000_000.0);
            assert!(
                (estimated - expected).abs() < 0.01,
                "{drift}: {estimated} vs {expected}"
            );
            let delay = tracker.estimate(now).unwrap();
            assert_eq!(delay.io_delay_ns, 30_000_000);
            assert!((delay.relative_rate_ppm.unwrap() - 2.0 * drift).abs() < 2.0);
        }
    }
    #[test]
    fn jitter_is_filtered_and_an_independent_clock_rate_step_is_tracked() {
        let mut clock = AudioClockTracker::default();
        let mut elapsed: f64 = 1_000_000_000.0;
        for (phase, drift) in [100.0, -750.0].into_iter().enumerate() {
            for i in 0..500 {
                elapsed += 240.0 * 1_000_000_000.0 / (48_000.0 * (1.0 + drift / 1_000_000.0));
                let frame = (phase * 500 + i + 1) as u64 * 240;
                let mut t = timestamp(StreamDirection::Input, frame, 48_000, 0.0, 20_000_000);
                let jitter = if i % 2 == 0 { 10_000 } else { -10_000 };
                t.estimated_time_ns = Some(elapsed.round() as i64 + jitter);
                t.observed_ns = (elapsed + 20_000_000.0) as u64;
                clock.observe_timestamp(t, false);
            }
            assert!((clock.rate_error_ppm().unwrap() - drift).abs() < 5.0);
        }
    }
    #[test]
    fn device_delay_step_tracks_without_confusing_chunk_offsets_or_repeated_polls() {
        let (mut tracker, _capture, _render) = controlled_tracker();
        for i in 0..80 {
            let c = timestamp(StreamDirection::Input, i * 480, 48_000, 0.0, 20_000_000);
            let mut r = timestamp(
                StreamDirection::Output,
                i * 480,
                48_000,
                0.0,
                if i < 10 { 10_000_000 } else { 40_000_000 },
            );
            // The second block of this callback starts 256 frames after the timestamp anchor.
            r.frame_position += 256;
            r.estimated_time_ns = r
                .estimated_time_ns
                .map(|ns| ns + 256 * 1_000_000_000 / 48_000);
            r.device_time_ns = r.device_time_ns.map(|ns| ns + 256 * 1_000_000_000 / 48_000);
            tracker.observe_timestamp(c, i == 0);
            tracker.observe_timestamp(r, i == 0);
            let estimate = tracker.estimate(c.observed_ns).unwrap();
            assert_eq!(estimate.capture_age_ns, 20_000_000);
            assert_eq!(
                estimate.render_lead_ns,
                if i < 10 { 10_000_000 } else { 40_000_000 }
            );
            assert_eq!(tracker.estimate(c.observed_ns), Some(estimate));
        }
        assert!((tracker.smoothed.unwrap() - 60_000_000.0).abs() < 30_000.0);
    }
    #[test]
    fn unknown_time_gaps_generation_changes_and_expiry_clear_estimates() {
        let (mut tracker, _capture, _render) = controlled_tracker();
        let c = timestamp(StreamDirection::Input, 0, 48_000, 0.0, 20_000_000);
        let r = timestamp(StreamDirection::Output, 0, 48_000, 0.0, 10_000_000);
        tracker.observe_timestamp(c, true);
        tracker.observe_timestamp(r, true);
        assert!(tracker.estimate(c.observed_ns).is_some());
        assert!(
            tracker
                .estimate(c.observed_ns + TAP_MAX_AGE_NS + 1)
                .is_none()
        );
        let mut unknown = c;
        unknown.estimated_time_ns = None;
        tracker.observe_timestamp(unknown, false);
        assert!(tracker.estimate(c.observed_ns).is_none());
        let mut next = c;
        next.stream_id += 10;
        next.frame_position = 480;
        tracker.observe_timestamp(next, true);
        assert_eq!(tracker.capture.points.iter().flatten().count(), 1);
        assert!(tracker.capture.rate_error_ppm().is_none());
        assert!(
            tracker
                .render_frame_for_capture(1_000_000, c.observed_ns)
                .is_none()
        );
    }
    #[test]
    fn stop_and_xrun_revoke_cached_alignment_without_new_blocks() {
        for close in [true, false] {
            let (mut tracker, capture, _render) = controlled_tracker();
            let c = timestamp(StreamDirection::Input, 0, 48_000, 0.0, 20_000_000);
            tracker.observe_timestamp(c, true);
            tracker.observe_timestamp(
                timestamp(StreamDirection::Output, 0, 48_000, 0.0, 10_000_000),
                true,
            );
            assert!(tracker.estimate(c.observed_ns).is_some());
            if close {
                capture.control().close();
            } else {
                capture.control().discontinue();
            }
            assert!(tracker.estimate(c.observed_ns).is_none());
            assert!(tracker.render_frame_for_capture(0, c.observed_ns).is_none());
        }
    }
}
