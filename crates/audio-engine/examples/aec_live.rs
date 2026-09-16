//! Finite, explicit-device acoustic smoke test. PCM stays in Rust; only metrics are printed.
//! Native stereo fixture -> incoming route -> speakers -> microphone -> AEC/DFN -> virtual
//! microphone -> independent CPAL receiver. Does not change defaults. Recording requires
//! the explicit --talk-record path option; it stores only the processed virtual microphone.
//! Successful exit means the sequence completed; review counters and listening separately.
use std::{
    error::Error,
    fs,
    io::{self, BufWriter, Write},
    num::NonZeroU16,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use audio_cpal::{CpalHost, DeviceSelection, NegotiationPreferences, StreamIssueStore};
use audio_engine::{
    AecDiagnostics, AecPrototypeConfig, AudioEngine, AudioEngineConfig, EngineCommand,
    HeadlessMonitor, HeadlessMonitorConfig, application_pcm_stream, bounded_audio_ring,
};
use audio_protocol::{
    ApplicationStreamId, AudioFormat, ChannelCount, DeviceId, DeviceKind, PipelineState,
    RouteConfig, RouteId, SampleFormat, SampleRateHz,
};

const RATE: usize = 48_000;
const CHANNELS: usize = 2;
const MAX_RECORDING_SAMPLES: usize = 20 * RATE * CHANNELS;

fn append_recording(recording: &mut Vec<i16>, samples: &[f32]) {
    let available = MAX_RECORDING_SAMPLES - recording.len();
    recording.extend(
        samples
            .iter()
            .take(available)
            .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16),
    );
}

fn write_recording(writer: &mut impl Write, samples: &[i16]) -> io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    writer.write_all(b"RIFF")?;
    writer.write_all(&(36 + data_len).to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16u32.to_le_bytes())?;
    writer.write_all(&1u16.to_le_bytes())?; // PCM i16, stereo, 48 kHz.
    writer.write_all(&(CHANNELS as u16).to_le_bytes())?;
    writer.write_all(&(RATE as u32).to_le_bytes())?;
    writer.write_all(&((RATE * CHANNELS * 2) as u32).to_le_bytes())?;
    writer.write_all(&((CHANNELS * 2) as u16).to_le_bytes())?;
    writer.write_all(&16u16.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        writer.write_all(&sample.to_le_bytes())?;
    }
    writer.flush()
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let recording_path = (args.len() == 6 && args[4] == "--talk-record").then(|| &args[5]);
    let talk = (args.len() == 5 && args[4] == "--talk") || recording_path.is_some();
    if args.len() != 4 && !talk {
        return Err("Usage: aec_live STEREO_48K.f32 SPEAKERS_ID MICROPHONE_ID VIRTUAL_MIC_ID [--talk | --talk-record NEW.wav]\nRuns three 20-second phases, or one AEC + DFN phase with --talk/--talk-record. Stay quiet for an echo-only comparison; speak during --talk. Keep speaker volume low; Ctrl-C stops. --talk-record explicitly saves up to 20 seconds of processed microphone PCM to a new private WAV after shutdown. No recording otherwise.".into());
    }
    let length = fs::metadata(&args[0])?.len();
    if length < (RATE * CHANNELS * 4) as u64
        || length > (60 * RATE * CHANNELS * 4) as u64
        || length % 8 != 0
    {
        return Err("fixture must be 1–60 seconds of stereo float32 LE at 48 kHz".into());
    }
    let samples: Vec<_> = fs::read(&args[0])?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes) * 0.5)
        .collect();
    if samples.iter().any(|x| !x.is_finite() || x.abs() > 0.25) {
        return Err("fixture must contain finite samples within ±0.5 before attenuation".into());
    }
    let speakers = DeviceId::new(args[1].clone())?;
    let microphone_id = DeviceId::new(args[2].clone())?;
    let virtual_mic = DeviceId::new(args[3].clone())?;
    if speakers == virtual_mic || microphone_id == virtual_mic {
        return Err(
            "speaker/microphone devices must be distinct from the virtual microphone".into(),
        );
    }
    let host = CpalHost::new_default();
    let inventory = host.inventory()?;
    for (id, input, kind) in [
        (&speakers, false, DeviceKind::Physical),
        (&microphone_id, true, DeviceKind::Physical),
        (&virtual_mic, true, DeviceKind::Virtual),
        (&virtual_mic, false, DeviceKind::Virtual),
    ] {
        if !inventory.devices.iter().any(|d| {
            &d.id == id
                && d.kind == kind
                && if input {
                    d.supports_input
                } else {
                    d.supports_output
                }
        }) {
            return Err(format!("required explicit device role unavailable: {id}").into());
        }
    }
    println!(
        "PATH: native fixture -> {speakers} -> {microphone_id} -> AEC/DFN -> {virtual_mic} -> native receiver"
    );
    let mut recording_file = if let Some(path) = recording_path {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        Some(BufWriter::new(options.open(path)?))
    } else {
        None
    };
    let mut recording = recording_path.map(|_| Vec::with_capacity(MAX_RECORDING_SAMPLES));
    println!("No system default changes; talk={talk}; recording={recording_path:?}.");
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Release))?;
    let _cancel_on_exit = CancelOnDrop(cancelled.clone());
    let phase = Arc::new(AtomicUsize::new(0));
    let feeder_drops = Arc::new(AtomicU64::new(0));
    let format = AudioFormat {
        sample_rate_hz: SampleRateHz::new(RATE as u32)?,
        channels: ChannelCount::new(CHANNELS as u16)?,
        sample_format: SampleFormat::F32,
        buffer_size_frames: None,
    };
    let (mut writer, source) = application_pcm_stream(format, 100)?;
    let mut engine = AudioEngine::start(AudioEngineConfig::default())?;
    let handle = engine.handle();
    let reference = RouteId::new("aec-live-incoming")?;
    handle.start_application_route(
        RouteConfig::application_stream(
            reference.clone(),
            ApplicationStreamId::new("aec-live-fixture")?,
            speakers,
            NonZeroU16::new(40).expect("constant"),
        ),
        source,
    )?;
    let feeder_cancel = cancelled.clone();
    let feeder_phase = phase.clone();
    let feeder_count = feeder_drops.clone();
    let feeder = thread::spawn(move || {
        let mut frame = [0.0; 960];
        let mut cursor = 0;
        let mut previous = 0;
        let mut next = Instant::now();
        while !feeder_cancel.load(Ordering::Acquire) {
            let current = feeder_phase.load(Ordering::Acquire);
            if current != previous {
                cursor = 0;
                previous = current;
            }
            if writer.is_ready() {
                for sample in &mut frame {
                    *sample = if current == 0 { 0.0 } else { samples[cursor] };
                    cursor = (cursor + 1) % samples.len();
                }
                if let Ok(report) = writer.write(&frame) {
                    feeder_count.fetch_add(report.dropped_frames as u64, Ordering::Relaxed);
                }
            }
            next += Duration::from_millis(10);
            thread::sleep(next.saturating_duration_since(Instant::now()));
            // Do not burst stale audio after a scheduler stall.
            if next.elapsed() > Duration::from_millis(20) {
                next = Instant::now();
            }
        }
    });
    let started = Instant::now();
    loop {
        let snapshot = handle.snapshot();
        if let Some(route) = snapshot.routes.iter().find(|r| r.config.id == reference) {
            if route.state == PipelineState::Running {
                println!(
                    "INCOMING_READY input={:?} output={:?}",
                    route.input_format, route.output_format
                );
                break;
            }
            if route.state == PipelineState::Failed {
                return Err(format!("incoming startup failed: {:?}", route.fault).into());
            }
        }
        if started.elapsed() > Duration::from_secs(30) || cancelled.load(Ordering::Acquire) {
            return Err("incoming startup timed out or cancelled".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    let receiver = host.resolve_input(&DeviceSelection::Id(virtual_mic.clone()))?;
    let receiver_config = receiver.negotiate_input(NegotiationPreferences::new(
        SampleRateHz::new(RATE as u32)?,
        ChannelCount::new(CHANNELS as u16)?,
    ))?;
    if receiver_config.protocol().sample_rate_hz.get() != RATE as u32
        || receiver_config.protocol().channels.get() != CHANNELS as u16
    {
        return Err("receiver requires stereo 48 kHz".into());
    }
    let (mut producer, mut consumer) = bounded_audio_ring(RATE / 5, ChannelCount::new(2)?)?;
    let receiver_drops = Arc::new(AtomicU64::new(0));
    let callback_drops = receiver_drops.clone();
    let receiver_stream = receiver.build_input_stream(
        &receiver_config,
        move |samples| {
            if let Ok(report) = producer.write(samples) {
                callback_drops.fetch_add(report.dropped_frames as u64, Ordering::Relaxed);
            }
        },
        StreamIssueStore::default(),
    )?;
    let mut buffer = [0.0; 4096];
    for (index, aec, bypass, label) in [
        (1, false, true, "BASELINE: AEC off, DFN bypassed"),
        (2, true, true, "AEC on, DFN bypassed"),
        (3, true, false, "AEC on, microphone and incoming DFN on"),
    ] {
        if talk && index != 3 {
            continue;
        }
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        phase.store(0, Ordering::Release);
        let diagnostics = AecDiagnostics::default();
        let mut microphone = HeadlessMonitor::start_with_cancellation(
            HeadlessMonitorConfig {
                route_id: RouteId::new("aec-live-microphone")?,
                audio_taps: handle.audio_taps(),
                input: DeviceSelection::Id(microphone_id.clone()),
                output: DeviceSelection::Id(virtual_mic.clone()),
                initially_bypassed: bypass,
                aec: aec.then(|| AecPrototypeConfig {
                    reference_route_id: Some(reference.clone()),
                    diagnostics: diagnostics.clone(),
                }),
                ..HeadlessMonitorConfig::default()
            },
            cancelled.clone(),
        )?;
        handle.try_send(EngineCommand::SetBypass {
            route_id: reference.clone(),
            bypassed: bypass,
        })?;
        receiver_stream.play()?;
        let instruction = if talk { "speak normally" } else { "stay quiet" };
        println!("PHASE {index}: {label}; {instruction} for 20 seconds");
        phase.store(index, Ordering::Release);
        let start = Instant::now();
        let mut next_report = Duration::from_secs(1);
        let mut sum = [0.0f64; 2];
        let mut frames = 0u64;
        while start.elapsed() < Duration::from_secs(20) && !cancelled.load(Ordering::Acquire) {
            microphone.poll_health();
            let read = consumer.read(&mut buffer)?;
            if let Some(recording) = &mut recording {
                // Owner thread only. The callback still copies to its bounded
                // 200 ms ring; no recording allocation or file I/O runs there.
                append_recording(recording, &buffer[..read.read_frames * CHANNELS]);
            }
            // Exclude startup/convergence; identical fixture offsets in each phase.
            if start.elapsed() >= Duration::from_secs(10) {
                for frame in buffer[..read.read_frames * CHANNELS].as_chunks::<2>().0 {
                    for channel in 0..2 {
                        sum[channel] += f64::from(frame[channel]).powi(2);
                    }
                }
                frames += read.read_frames as u64;
            }
            if start.elapsed() >= next_report {
                println!(
                    "PHASE={index} SECOND={} AEC={:?} MIC={:?}",
                    start.elapsed().as_secs(),
                    diagnostics.snapshot(),
                    microphone.snapshot()
                );
                next_report += Duration::from_secs(1);
            }
            thread::sleep(Duration::from_millis(5));
        }
        phase.store(0, Ordering::Release);
        println!(
            "RESULT phase={index} receiver_frames={frames} rms_left={} rms_right={} receiver_drops={} feeder_drops={} AEC={:?}",
            (sum[0] / frames.max(1) as f64).sqrt(),
            (sum[1] / frames.max(1) as f64).sqrt(),
            receiver_drops.load(Ordering::Relaxed),
            feeder_drops.load(Ordering::Relaxed),
            diagnostics.snapshot()
        );
        println!("MIC_FINAL={:?}", microphone.snapshot());
        println!(
            "RECEIVER callbacks={:?} issues={:?}",
            receiver_stream.callback_metrics(),
            receiver_stream.issue_snapshot()
        );
        println!("INCOMING={:?}", handle.snapshot());
        microphone.stop()?;
        receiver_stream.pause()?;
        while consumer.read(&mut buffer)?.read_frames != 0 {}
        println!("STOPPED phase={index} AEC={:?}", diagnostics.snapshot());
    }
    engine.shutdown()?;
    cancelled.store(true, Ordering::Release);
    feeder.join().map_err(|_| "fixture feeder panicked")?;
    println!("All test routes stopped.");
    if let (Some(file), Some(recording), Some(path)) =
        (&mut recording_file, &recording, recording_path)
    {
        write_recording(file, recording)?;
        println!(
            "SAVED processed_microphone={path} frames={}; delete after listening review.",
            recording.len() / CHANNELS
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_is_bounded_and_preserves_channel_order() {
        let mut recording = vec![0; MAX_RECORDING_SAMPLES - 2];
        append_recording(&mut recording, &[1.0, -1.0, 0.5, -0.5]);
        assert_eq!(recording.len(), MAX_RECORDING_SAMPLES);
        assert_eq!(&recording[recording.len() - 2..], &[32767, -32767]);
        append_recording(&mut recording, &[0.25, 0.25]);
        assert_eq!(recording.len(), MAX_RECORDING_SAMPLES);
    }

    #[test]
    fn recording_wav_has_exact_lengths_format_and_pcm() {
        let mut wav = Vec::new();
        write_recording(&mut wav, &[123, -456, 789, -1024]).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize,
            wav.len() - 8
        );
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(&wav[20..24], &[1, 0, 2, 0]);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 48_000);
        assert_eq!(&wav[32..36], &[4, 0, 16, 0]);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(&wav[44..], &[123, 0, 56, 254, 21, 3, 0, 252]);
    }
}
