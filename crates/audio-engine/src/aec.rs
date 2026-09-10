//! Opt-in, worker-confined AEC3 prototype. Nothing here is a Tauri command or PCM IPC type.
use crate::AudioClockTracker;
use aec_webrtc::{Aec3, FRAME_SAMPLES, Frame};
use audio_cpal::{
    AudioTapControl, AudioTapReader, AudioTimestamp, MonotonicClock, TAP_CAPACITY_BLOCKS,
    TAP_MAX_AGE_NS, TimedAudioBlock,
};
use audio_dsp::FixedDryDelay;
use audio_protocol::RouteId;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    mpsc::{Receiver, SyncSender},
};
use std::time::{Duration, Instant};

/// Measured impulse peak for this pinned APM configuration, not device/end-to-end latency.
pub const AEC_DELAY_SAMPLES: usize = 430;
const HISTORY_FRAMES: usize = 32768;
const TAPS: usize = 32;
const PHASES: usize = 128;
const MAX_RELATIVE_PPM: f64 = 1000.0;

#[derive(Clone, Debug)]
pub struct AecPrototypeConfig {
    /// Only the incoming route slot can supply this reference; never the microphone output.
    pub reference_route_id: RouteId,
    pub diagnostics: AecDiagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AecStatus {
    WaitingReference,
    Aligning,
    Running,
    InvalidTiming,
    NativeError,
    Overloaded,
    Stopped,
}

#[derive(Debug)]
struct Atoms {
    status: AtomicU8,
    needs_reference: AtomicBool,
    processed: AtomicU64,
    fallback: AtomicU64,
    resets: AtomicU64,
    errors: AtomicU64,
    max_us: AtomicU64,
    deadlines: AtomicU64,
}
impl Default for Atoms {
    fn default() -> Self {
        Self {
            status: AtomicU8::new(AecStatus::WaitingReference as u8),
            needs_reference: AtomicBool::new(true),
            processed: AtomicU64::new(0),
            fallback: AtomicU64::new(0),
            resets: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
            deadlines: AtomicU64::new(0),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct AecDiagnostics(Arc<Atoms>);
#[derive(Clone, Copy, Debug)]
pub struct AecSnapshot {
    pub status: AecStatus,
    pub processed_frames: u64,
    pub fallback_frames: u64,
    pub resets: u64,
    pub native_errors: u64,
    pub max_processing_us: u64,
    pub missed_deadlines: u64,
}
impl AecDiagnostics {
    pub fn snapshot(&self) -> AecSnapshot {
        let a = &self.0;
        let status = match a.status.load(Ordering::Acquire) {
            0 => AecStatus::WaitingReference,
            1 => AecStatus::Aligning,
            2 => AecStatus::Running,
            3 => AecStatus::InvalidTiming,
            4 => AecStatus::NativeError,
            5 => AecStatus::Overloaded,
            _ => AecStatus::Stopped,
        };
        AecSnapshot {
            status,
            processed_frames: a.processed.load(Ordering::Relaxed),
            fallback_frames: a.fallback.load(Ordering::Relaxed),
            resets: a.resets.load(Ordering::Relaxed),
            native_errors: a.errors.load(Ordering::Relaxed),
            max_processing_us: a.max_us.load(Ordering::Relaxed),
            missed_deadlines: a.deadlines.load(Ordering::Relaxed),
        }
    }
    fn status(&self, status: AecStatus) {
        self.0.status.store(status as u8, Ordering::Release);
    }
    pub(crate) fn needs_reference(&self) -> bool {
        self.0.needs_reference.load(Ordering::Acquire)
    }
    pub(crate) fn reference_sent(&self) {
        self.0.needs_reference.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Default)]
struct Sample {
    value: [f32; 2],
    observed_ns: u64,
}

/// Single resampling stage from timestamp-indexed native frames to the capture's 48 kHz grid.
/// 32-tap, 128-phase windowed sinc. Storage is fixed; original callback age is retained.
struct ReferenceHistory {
    samples: Vec<Sample>,
    coefficients: Vec<[f32; TAPS]>,
    start: u64,
    end: u64,
    latest: Option<AudioTimestamp>,
    clock: AudioClockTracker,
}
impl ReferenceHistory {
    fn new() -> Self {
        Self {
            samples: vec![Sample::default(); HISTORY_FRAMES],
            coefficients: vec![[0.0; TAPS]; PHASES],
            start: 0,
            end: 0,
            latest: None,
            clock: AudioClockTracker::default(),
        }
    }
    fn reset(&mut self) {
        self.start = 0;
        self.end = 0;
        self.latest = None;
        self.clock.reset();
    }
    fn configure(&mut self, rate: u32) {
        let cutoff = (48000.0 / f64::from(rate)).min(1.0) * 0.9;
        for (phase, coefficients) in self.coefficients.iter_mut().enumerate() {
            let fraction = phase as f64 / PHASES as f64;
            let mut sum = 0.0;
            for (i, coefficient) in coefficients.iter_mut().enumerate() {
                let x = i as f64 - (TAPS / 2 - 1) as f64 - fraction;
                let z = std::f64::consts::PI * x * cutoff;
                let sinc = if z.abs() < 1e-12 { 1.0 } else { z.sin() / z };
                let window = 0.5 + 0.5 * (std::f64::consts::PI * x / (TAPS / 2) as f64).cos();
                *coefficient = (cutoff * sinc * window) as f32;
                sum += *coefficient;
            }
            for c in coefficients {
                *c /= sum;
            }
        }
    }
    // Returns false on invalid metadata/content. A discontinuity always discards cached PCM.
    fn push(&mut self, t: AudioTimestamp, discontinuity: bool, pcm: &[f32]) -> bool {
        let channels = usize::from(t.channels);
        if !(1..=2).contains(&channels)
            || !(8000..=96000).contains(&t.sample_rate_hz)
            || pcm.is_empty()
            || !pcm.len().is_multiple_of(channels)
            || pcm.iter().any(|x| !x.is_finite() || x.abs() > 1.0)
            || t.device_time_ns.is_none()
            || t.estimated_time_ns.is_none()
        {
            self.reset();
            return false;
        }
        let changed = self.latest.is_none_or(|last| {
            last.stream_id != t.stream_id
                || last.epoch != t.epoch
                || last.sample_rate_hz != t.sample_rate_hz
                || last.channels != t.channels
        });
        if changed || discontinuity || t.frame_position != self.end {
            self.reset();
            self.configure(t.sample_rate_hz);
            self.start = t.frame_position;
            self.end = t.frame_position;
        }
        self.clock.observe_timestamp(t, discontinuity);
        for frame in pcm.chunks_exact(channels) {
            self.samples[self.end as usize % HISTORY_FRAMES] = Sample {
                value: [frame[0], if channels == 2 { frame[1] } else { 0.0 }],
                observed_ns: t.observed_ns,
            };
            self.end += 1;
            self.start = self
                .start
                .max(self.end.saturating_sub(HISTORY_FRAMES as u64));
        }
        self.latest = Some(t);
        true
    }
    fn sample(&self, position: f64, now: u64) -> Option<[f32; 2]> {
        if !position.is_finite() || position < (TAPS / 2 - 1) as f64 {
            return None;
        }
        let first = position.floor() as u64 - (TAPS / 2 - 1) as u64;
        if first < self.start || first.checked_add(TAPS as u64)? > self.end {
            return None;
        }
        let phase = ((position.fract() * PHASES as f64) as usize).min(PHASES - 1);
        let mut out = [0.0; 2];
        for (i, c) in self.coefficients[phase].iter().enumerate() {
            let s = self.samples[(first as usize + i) % HISTORY_FRAMES];
            if now.saturating_sub(s.observed_ns) > TAP_MAX_AGE_NS {
                return None;
            }
            for (o, v) in out.iter_mut().zip(s.value) {
                *o += c * v;
            }
        }
        Some(out.map(|x| x.clamp(-1.0, 1.0)))
    }
}

pub(crate) struct AecStage {
    diagnostics: AecDiagnostics,
    reader: Option<AudioTapReader>,
    incoming: Receiver<AudioTapReader>,
    returned: SyncSender<AudioTapReader>,
    capture_control: AudioTapControl,
    capture_clock: AudioClockTracker,
    capture: Option<AudioTimestamp>,
    origin: Option<u64>,
    emitted: u64,
    input_delay: usize,
    history: ReferenceHistory,
    native: Option<Aec3>,
    dry_delay: FixedDryDelay,
    dry: Frame,
    wet: Frame,
    render: [Frame; 2],
    blend: f32,
    cooldown: usize,
    clock: MonotonicClock,
    #[cfg(test)]
    test_now: Option<u64>,
}
impl AecStage {
    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> AecSnapshot {
        self.diagnostics.snapshot()
    }
    pub(crate) fn new(
        diagnostics: AecDiagnostics,
        capture_control: AudioTapControl,
        input_delay: usize,
        incoming: Receiver<AudioTapReader>,
        returned: SyncSender<AudioTapReader>,
    ) -> Result<Self, audio_dsp::DspError> {
        diagnostics.0.needs_reference.store(true, Ordering::Release);
        diagnostics.status(AecStatus::WaitingReference);
        Ok(Self {
            diagnostics,
            reader: None,
            incoming,
            returned,
            capture_control,
            capture_clock: AudioClockTracker::default(),
            capture: None,
            origin: None,
            emitted: 0,
            input_delay,
            history: ReferenceHistory::new(),
            native: None,
            dry_delay: FixedDryDelay::new(
                audio_dsp::ChannelLayout::Mono.channels(),
                AEC_DELAY_SAMPLES,
            )?,
            dry: [0.0; FRAME_SAMPLES],
            wet: [0.0; FRAME_SAMPLES],
            render: [[0.0; FRAME_SAMPLES]; 2],
            blend: 0.0,
            cooldown: 0,
            clock: MonotonicClock::default(),
            #[cfg(test)]
            test_now: None,
        })
    }
    fn invalidate_native(&mut self) {
        if self.native.take().is_some() {
            self.diagnostics.0.resets.fetch_add(1, Ordering::Relaxed);
        }
        self.blend = 0.0;
    }
    pub(crate) fn reset_capture(&mut self) {
        self.invalidate_native();
        self.capture_clock.reset();
        self.capture = None;
        self.origin = None;
        self.emitted = 0;
        self.dry_delay.reset();
        self.history.reset();
    }
    pub(crate) fn observe_capture(&mut self, block: &TimedAudioBlock) {
        self.origin.get_or_insert(block.timestamp.frame_position);
        self.capture_clock.observe(block);
        self.capture = Some(block.timestamp);
    }
    fn drain_reference(&mut self) {
        if self.reader.as_ref().is_some_and(AudioTapReader::is_closed) {
            self.reader = None;
            self.history.reset();
            self.invalidate_native();
            self.diagnostics
                .0
                .needs_reference
                .store(true, Ordering::Release);
        }
        if self.reader.is_none()
            && let Ok(reader) = self.incoming.try_recv()
        {
            self.reader = Some(reader);
        }
        for _ in 0..TAP_CAPACITY_BLOCKS {
            let Some(reader) = &mut self.reader else {
                break;
            };
            let Some(block) = reader.read() else {
                break;
            };
            if !reader.is_current(&block) {
                continue;
            }
            if block.discontinuity {
                self.invalidate_native();
            }
            if !self
                .history
                .push(block.timestamp, block.discontinuity, block.samples())
            {
                self.invalidate_native();
            }
        }
    }
    fn align(&mut self, frame_start: u64, now: u64) -> Result<usize, AecStatus> {
        let reader = self.reader.as_ref().ok_or(AecStatus::WaitingReference)?;
        let c = self.capture.ok_or(AecStatus::Aligning)?;
        let r = self.history.latest.ok_or(AecStatus::Aligning)?;
        if !self.capture_control.is_active()
            || self.capture_control.epoch() != c.epoch
            || !reader.is_active()
            || reader.control().epoch() != r.epoch
            || now.saturating_sub(c.observed_ns) > TAP_MAX_AGE_NS
            || now.saturating_sub(r.observed_ns) > TAP_MAX_AGE_NS
        {
            return Err(AecStatus::InvalidTiming);
        }
        let capture_ppm = self
            .capture_clock
            .rate_error_ppm()
            .ok_or(AecStatus::Aligning)?;
        let render_ppm = self
            .history
            .clock
            .rate_error_ppm()
            .ok_or(AecStatus::Aligning)?;
        let relative_ppm = ((1.0 + render_ppm / 1e6) / (1.0 + capture_ppm / 1e6) - 1.0) * 1e6;
        if relative_ppm.abs() > MAX_RELATIVE_PPM {
            return Err(AecStatus::InvalidTiming);
        }
        let canonical = frame_start
            .checked_sub(self.input_delay as u64)
            .ok_or(AecStatus::Aligning)?;
        let origin = self.origin.ok_or(AecStatus::Aligning)?;
        let native_start = origin as f64 + canonical as f64 * f64::from(c.sample_rate_hz) / 48000.0;
        let native_end =
            native_start + (FRAME_SAMPLES - 1) as f64 * f64::from(c.sample_rate_hz) / 48000.0;
        let time = |position: f64| -> Option<i64> {
            let low = self
                .capture_clock
                .time_at_frame(position.floor() as u64, now)?;
            let high = self
                .capture_clock
                .time_at_frame(position.floor() as u64 + 1, now)?;
            Some(low + ((high - low) as f64 * position.fract()).round() as i64)
        };
        let first = self
            .history
            .clock
            .frame_at_time(time(native_start).ok_or(AecStatus::InvalidTiming)?, now)
            .ok_or(AecStatus::InvalidTiming)?;
        let last = self
            .history
            .clock
            .frame_at_time(time(native_end).ok_or(AecStatus::InvalidTiming)?, now)
            .ok_or(AecStatus::InvalidTiming)?;
        let step = (last - first) / (FRAME_SAMPLES - 1) as f64;
        for i in 0..FRAME_SAMPLES {
            let sample = self
                .history
                .sample(first + i as f64 * step, now)
                .ok_or(AecStatus::Aligning)?;
            self.render[0][i] = sample[0];
            self.render[1][i] = sample[1];
        }
        Ok(usize::from(r.channels))
    }
    /// Always emits one microphone frame. Missing reference never stalls capture or its clock.
    pub(crate) fn process(&mut self, input: &[f32], output: &mut Frame, allowed: bool) -> Duration {
        let now = self.clock.now_ns();
        #[cfg(test)]
        let now = self.test_now.unwrap_or(now);
        self.process_at(input, output, allowed, now)
    }
    fn process_at(
        &mut self,
        input: &[f32],
        output: &mut Frame,
        allowed: bool,
        now: u64,
    ) -> Duration {
        let started = Instant::now();
        let frame_start = self.emitted;
        self.emitted += FRAME_SAMPLES as u64;
        // Graph construction validates the exact mono 480-sample contract.
        let mut capture = [0.0; FRAME_SAMPLES];
        for (out, sample) in capture.iter_mut().zip(input) {
            *out = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        if self.dry_delay.process(&capture, &mut self.dry).is_err() {
            self.dry.fill(0.0);
        }
        self.drain_reference();
        let result = if !allowed || self.cooldown > 0 {
            self.cooldown = self.cooldown.saturating_sub(1);
            Err(AecStatus::Overloaded)
        } else {
            self.align(frame_start, now)
        };
        let status = match result {
            Ok(channels) => {
                let initialized = if self.native.is_none() {
                    Aec3::new(channels).map(|native| self.native = Some(native))
                } else {
                    Ok(())
                };
                let result = initialized.and_then(|()| {
                    let native = self
                        .native
                        .as_mut()
                        .ok_or(aec_webrtc::AecError::Initialization)?;
                    native.analyze_render(&self.render[..channels])?;
                    native.process_capture(&capture, &mut self.wet)
                });
                if result.is_ok() {
                    AecStatus::Running
                } else {
                    self.diagnostics.0.errors.fetch_add(1, Ordering::Relaxed);
                    self.cooldown = 100;
                    AecStatus::NativeError
                }
            }
            Err(status) => status,
        };
        if status == AecStatus::Running {
            self.diagnostics.0.processed.fetch_add(1, Ordering::Relaxed);
            for ((out, dry), wet) in output.iter_mut().zip(self.dry).zip(self.wet) {
                self.blend = (self.blend + 1.0 / 960.0).min(1.0);
                *out = dry + self.blend * (wet - dry);
            }
        } else {
            self.invalidate_native();
            self.diagnostics.0.fallback.fetch_add(1, Ordering::Relaxed);
            output.copy_from_slice(&self.dry);
        }
        self.diagnostics.status(status);
        let elapsed = started.elapsed();
        self.diagnostics.0.max_us.fetch_max(
            elapsed.as_micros().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
        if elapsed > Duration::from_millis(10) {
            self.diagnostics.0.deadlines.fetch_add(1, Ordering::Relaxed);
            self.cooldown = 100;
        }
        elapsed
    }
}
impl Drop for AecStage {
    fn drop(&mut self) {
        self.diagnostics.status(AecStatus::Stopped);
        // Worker returns ownership through a bounded channel; registry mutex is owner-thread only.
        let reader = self
            .reader
            .take()
            .filter(|reader| !reader.is_closed())
            .or_else(|| self.incoming.try_recv().ok());
        if let Some(reader) = reader {
            let _ = self.returned.try_send(reader);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use audio_cpal::{StreamDirection, audio_tap};
    use audio_protocol::{AudioFormat, ChannelCount, SampleFormat, SampleRateHz};
    use std::sync::mpsc::sync_channel;

    fn stamp(frame: u64, rate: u32, ppm: f64, direction: StreamDirection) -> AudioTimestamp {
        let ns = 1_000_000_000
            + (frame as f64 * 1e9 / (f64::from(rate) * (1.0 + ppm / 1e6))).round() as i64;
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
            channels: 2,
            direction,
            observed_ns: ns as u64,
            device_time_ns: Some(
                (ns + if direction == StreamDirection::Input {
                    50_000_000_000
                } else {
                    90_000_000_000
                }) as u64,
            ),
            estimated_time_ns: Some(ns),
        }
    }
    fn tap(direction: StreamDirection) -> (audio_cpal::AudioTapWriter, AudioTapReader) {
        audio_tap(
            AudioFormat {
                sample_rate_hz: SampleRateHz::new(48000).unwrap(),
                channels: ChannelCount::new(2).unwrap(),
                sample_format: SampleFormat::F32,
                buffer_size_frames: None,
            },
            direction,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    }
    pub(crate) fn stage() -> (
        AecStage,
        audio_cpal::AudioTapWriter,
        audio_cpal::AudioTapWriter,
        Receiver<AudioTapReader>,
    ) {
        let (capture, _) = tap(StreamDirection::Input);
        let (render, reader) = tap(StreamDirection::Output);
        let (_, incoming) = sync_channel(1);
        let (returned, rx) = sync_channel(1);
        let mut stage = AecStage::new(
            AecDiagnostics::default(),
            capture.control(),
            0,
            incoming,
            returned,
        )
        .unwrap();
        stage.reader = Some(reader);
        (stage, capture, render, rx)
    }
    fn tone(time: f64) -> [f32; 2] {
        [
            (time * 2.0 * std::f64::consts::PI * 437.0).sin() as f32 * 0.3,
            (time * 2.0 * std::f64::consts::PI * 773.0).cos() as f32 * 0.2,
        ]
    }
    /// Deterministic backend clock, no device opened. Render is submitted one frame ahead.
    pub(crate) fn tick(aec: &mut AecStage, frame: u64) {
        let c = stamp(frame * 480, 48000, 0.0, StreamDirection::Input);
        aec.capture_clock.observe_timestamp(c, frame == 0);
        aec.capture = Some(c);
        aec.origin.get_or_insert(0);
        let r = stamp((frame + 1) * 480, 48000, 0.0, StreamDirection::Output);
        let mut pcm = [0.0; 960];
        for (i, samples) in pcm.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            samples.copy_from_slice(&tone(((frame + 1) * 480 + i as u64) as f64 / 48000.0));
        }
        aec.history.push(r, frame == 0, &pcm);
        aec.test_now = Some(r.observed_ns);
    }
    #[test]
    fn stop_before_worker_acquires_handoff_returns_the_pending_reader() {
        let (capture, _) = tap(StreamDirection::Input);
        let (_render, reader) = tap(StreamDirection::Output);
        let (tx, incoming) = sync_channel(1);
        let (returned, rx) = sync_channel(1);
        let stage = AecStage::new(
            AecDiagnostics::default(),
            capture.control(),
            0,
            incoming,
            returned,
        )
        .unwrap();
        assert!(tx.try_send(reader).is_ok());
        drop(stage);
        assert!(rx.try_recv().unwrap().is_active());
    }
    fn feed(stage: &mut AecStage, capture_rate: u32, render_rate: u32, ppm: f64) -> u64 {
        for i in 0..50 {
            let frame = i * u64::from(capture_rate) / 200;
            let t = stamp(frame, capture_rate, 0.0, StreamDirection::Input);
            stage.capture_clock.observe_timestamp(t, i == 0);
            stage.capture = Some(t);
        }
        stage.origin = Some(0);
        for i in 0..60 {
            let frame = i * u64::from(render_rate) / 200;
            let end = (i + 1) * u64::from(render_rate) / 200;
            let pcm: Vec<f32> = (frame..end)
                .flat_map(|p| tone(p as f64 / (f64::from(render_rate) * (1.0 + ppm / 1e6))))
                .collect();
            stage.history.push(
                stamp(frame, render_rate, ppm, StreamDirection::Output),
                i == 0,
                &pcm,
            );
        }
        1_300_000_000
    }
    #[test]
    fn resamples_stereo_on_independent_capture_clock_and_preserves_channel_order() {
        for capture_rate in [44100, 48000, 96000] {
            for render_rate in [44100, 48000, 96000] {
                for ppm in [-750.0, -100.0, 100.0, 750.0] {
                    let (mut aec, _c, _r, _) = stage();
                    let now = feed(&mut aec, capture_rate, render_rate, ppm);
                    assert_eq!(aec.align(9600, now), Ok(2));
                    let aligned = aec.render;
                    aec.input_delay = 123;
                    assert_eq!(aec.align(9723, now), Ok(2));
                    assert_eq!(
                        aec.render, aligned,
                        "canonical input-resampler delay must be subtracted"
                    );
                    for i in 0..FRAME_SAMPLES {
                        let expected = tone((9600 + i) as f64 / 48000.0);
                        assert!(
                            (aec.render[0][i] - expected[0]).abs() < 0.001,
                            "capture={capture_rate}, render={render_rate}, ppm={ppm}"
                        );
                        assert!((aec.render[1][i] - expected[1]).abs() < 0.001);
                    }
                }
            }
        }
    }
    #[test]
    fn retained_pcm_is_bounded_and_keeps_original_age_and_antiphase_channels() {
        let mut h = ReferenceHistory::new();
        for i in 0..300 {
            let t = stamp(i * 256, 96000, 0.0, StreamDirection::Output);
            assert!(h.push(t, i == 0, &[0.25, -0.25].repeat(256)));
        }
        assert_eq!(h.end - h.start, HISTORY_FRAMES as u64);
        assert_eq!(h.samples.len(), HISTORY_FRAMES);
        let now = h.latest.unwrap().observed_ns;
        let frame = h.end - 100;
        let sample = h.sample(frame as f64 + 0.5, now).unwrap();
        assert!((sample[0] - 0.25).abs() < 1e-6);
        assert!((sample[0] + sample[1]).abs() < 1e-6);
        assert!(
            h.sample((h.start + 16) as f64, now).is_none(),
            "retained but expired audio stays unusable"
        );
        assert!(h.sample(frame as f64, now + TAP_MAX_AGE_NS + 1).is_none());
        let mut next = stamp(h.end, 96000, 0.0, StreamDirection::Output);
        next.epoch = 1;
        h.push(next, true, &[0.0; 512]);
        assert!(h.sample(frame as f64, next.observed_ns).is_none());
        assert_eq!(
            h.sample((next.frame_position + 32) as f64, next.observed_ns),
            Some([0.0, 0.0])
        );
    }
    #[test]
    fn stop_xrun_expiry_unknown_clock_and_excess_drift_revoke_learned_state() {
        for fault in 0..7 {
            let (mut aec, capture, render, _) = stage();
            let now = feed(&mut aec, 48000, 48000, 100.0);
            aec.emitted = 9600;
            let mut output = [0.0; FRAME_SAMPLES];
            aec.process_at(&[0.1; FRAME_SAMPLES], &mut output, true, now);
            assert!(aec.native.is_some());
            let mut next_now = now;
            match fault {
                0 => render.control().close(),
                1 => render.control().discontinue(),
                2 => capture.control().discontinue(),
                3 => next_now += TAP_MAX_AGE_NS + 1,
                4 => aec.capture_clock.reset(),
                5 => {
                    feed(&mut aec, 48000, 48000, 1500.0);
                }
                _ => {}
            }
            aec.process_at(&[0.1; FRAME_SAMPLES], &mut output, fault != 6, next_now);
            assert!(aec.native.is_none(), "fault {fault}");
            assert_ne!(aec.diagnostics.snapshot().status, AecStatus::Running);
            assert!(output.iter().all(|x| (*x - 0.1).abs() < 1e-6));
            assert_eq!(aec.diagnostics.snapshot().resets, 1);
        }
    }
    #[test]
    fn missing_reference_preserves_duration_and_impulse_delay_and_reset_clears_microphone_history()
    {
        let (mut aec, _c, _r, rx) = stage();
        let mut input = [0.0; FRAME_SAMPLES];
        input[0] = 0.5;
        let mut output = [0.0; FRAME_SAMPLES];
        let info = allocation_counter::measure(|| {
            aec.process_at(&input, &mut output, true, 0);
        });
        assert_eq!(info.count_total, 0);
        assert_eq!(output[AEC_DELAY_SAMPLES], 0.5);
        assert_eq!(output.iter().filter(|x| **x != 0.0).count(), 1);
        aec.process_at(&[0.5; FRAME_SAMPLES], &mut output, false, 0);
        aec.reset_capture();
        aec.process_at(&[0.0; FRAME_SAMPLES], &mut output, true, 0);
        assert_eq!(output, [0.0; FRAME_SAMPLES]);
        let diagnostics = aec.diagnostics.clone();
        drop(aec);
        assert!(rx.try_recv().unwrap().is_active());
        assert_eq!(diagnostics.snapshot().status, AecStatus::Stopped);
    }
}
