//! Finite macOS hardware check using the production Phase 6 route and synthetic playback.
//! Usage: macos_incoming_listening INPUT_STEREO_48K.f32 PHYSICAL_OUTPUT_ID
//! Does not change system defaults or open a physical microphone. Ctrl-C stops all streams.
use std::{
    error::Error,
    fs, io,
    num::NonZeroU16,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use audio_cpal::{CpalHost, DeviceSelection, NegotiationPreferences, StreamIssueStore};
use audio_engine::{AudioEngine, AudioEngineConfig, AudioEngineHandle, EngineCommand};
use audio_protocol::{
    ChannelCount, DenoiseSettings, DestinationEndpoint, DeviceId, DeviceKind, LoopbackEndpointId,
    PipelineState, ProcessingMode, RouteConfig, RouteId, RouteSnapshot, SampleRateHz,
    SourceEndpoint,
};

const RATE: usize = 48_000;
const CHANNELS: usize = 2;
const BLACKHOLE: &str = "coreaudio:BlackHole2ch_UID";

fn main() -> Result<(), Box<dyn Error>> {
    if !cfg!(target_os = "macos") {
        return Err(io::Error::other("this check requires macOS and BlackHole 2ch").into());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(io::Error::other(
            "Usage: macos_incoming_listening INPUT_STEREO_48K.f32 PHYSICAL_OUTPUT_ID",
        )
        .into());
    }
    let length = fs::metadata(&args[0])?.len();
    if length == 0 || length > (60 * RATE * CHANNELS * 4) as u64 || length % 8 != 0 {
        return Err(
            io::Error::other("expected 1–60 seconds of stereo float32 LE at 48 kHz").into(),
        );
    }
    let bytes = fs::read(&args[0])?;
    let samples: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|value| f32::from_le_bytes(*value))
        .collect();
    if samples.len() < RATE * CHANNELS
        || samples
            .iter()
            .any(|value| !value.is_finite() || value.abs() > 0.5)
    {
        return Err(io::Error::other("fixture must be at least one second and within ±0.5").into());
    }
    let fixture_frames = samples.len() / CHANNELS;
    let output_id = DeviceId::new(args[1].clone())?;
    let host = CpalHost::new_default();
    let inventory = host.inventory()?;
    let output = inventory
        .devices
        .iter()
        .find(|device| device.id == output_id && device.supports_output)
        .ok_or_else(|| io::Error::other("explicit output device unavailable"))?;
    if output.kind != DeviceKind::Physical {
        return Err(io::Error::other("output must be verified physical headphones").into());
    }
    println!("INPUT={BLACKHOLE}; OUTPUT={} ({})", output.name, output.id);
    println!("SOURCE=synthetic fixture; RATE={RATE}; CHANNELS={CHANNELS}; FRAMES={fixture_frames}");
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal_cancelled = cancelled.clone();
    ctrlc::set_handler(move || signal_cancelled.store(true, Ordering::Release))?;

    // Only BlackHole is opened for fixture playback; capture is owned by the real engine route.
    let playback = host.resolve_output(&DeviceSelection::Id(DeviceId::new(BLACKHOLE)?))?;
    let playback_config = playback.negotiate_output(NegotiationPreferences::new(
        SampleRateHz::new(RATE as u32)?,
        ChannelCount::new(CHANNELS as u16)?,
    ))?;
    if playback_config.protocol().sample_rate_hz.get() != RATE as u32
        || playback_config.protocol().channels.get() != CHANNELS as u16
    {
        return Err(
            io::Error::other("BlackHole must support stereo 48 kHz for this fixture").into(),
        );
    }
    let phase = Arc::new(AtomicUsize::new(0));
    let played = Arc::new(AtomicUsize::new(0));
    let callback_phase = phase.clone();
    let callback_played = played.clone();
    let callback_cancelled = cancelled.clone();
    let mut previous_phase = 0;
    let mut cursor = 0;
    let stream = playback.build_output_stream(
        &playback_config,
        move |destination| {
            destination.fill(0.0);
            if callback_cancelled.load(Ordering::Acquire) {
                return;
            }
            let current = callback_phase.load(Ordering::Acquire);
            if current != previous_phase {
                cursor = 0;
                previous_phase = current;
            }
            if current == 0 {
                callback_played.store(0, Ordering::Release);
                return;
            }
            for frame in destination.as_chunks_mut::<CHANNELS>().0 {
                if current == 3 {
                    // A quiet 440 Hz tone in the left channel, then 660 Hz in the right.
                    let seconds = cursor as f32 / RATE as f32;
                    let (channel, start, frequency) = if seconds < 1.5 {
                        (0, 0.5, 440.0)
                    } else {
                        (1, 2.0, 660.0)
                    };
                    let local = seconds - start;
                    if (0.0..0.5).contains(&local) {
                        let envelope = (local * 100.0).min((0.5 - local) * 100.0).min(1.0);
                        frame[channel] =
                            0.06 * envelope * (std::f32::consts::TAU * frequency * local).sin();
                    }
                } else if cursor < fixture_frames {
                    // Fixed attenuation for comfortable initial playback; identical for both modes.
                    frame[0] = samples[cursor * CHANNELS] * 0.5;
                    frame[1] = samples[cursor * CHANNELS + 1] * 0.5;
                }
                cursor = cursor.saturating_add(1);
            }
            callback_played.store(cursor, Ordering::Release);
        },
        StreamIssueStore::default(),
    )?;

    let mut engine = AudioEngine::start(AudioEngineConfig::default())?;
    let handle = engine.handle();
    let route_id = RouteId::new("phase6-macos-listening")?;
    handle.try_send(EngineCommand::StartRoute(RouteConfig {
        id: route_id.clone(),
        source: SourceEndpoint::SystemLoopback(LoopbackEndpointId::new("macos:blackhole-2ch")?),
        destination: DestinationEndpoint::PhysicalOutput(output_id.clone()),
        echo_cancellation: false,
        processing_mode: ProcessingMode::AlignedBypass,
        target_latency_ms: NonZeroU16::new(40).expect("nonzero constant"),
        denoise: DenoiseSettings::default(),
    }))?;
    let initial = wait_for(&handle, &route_id, &cancelled, |route| {
        route.state == PipelineState::Running && route.bypassed
    })?;
    if initial
        .input_device
        .as_ref()
        .map(|device| device.id.as_str())
        != Some(BLACKHOLE)
        || initial.output_device.as_ref().map(|device| &device.id) != Some(&output_id)
    {
        return Err(io::Error::other("engine opened unexpected devices").into());
    }
    println!(
        "NEGOTIATED_INPUT={:?}; NEGOTIATED_OUTPUT={:?}; MODEL={:?}",
        initial.input_format, initial.output_format, initial.model
    );
    stream.play()?;
    let mut summaries = Vec::new();
    for (number, bypassed, label, frames) in [
        (1, true, "NOISE REDUCTION OFF", fixture_frames),
        (2, false, "NOISE REDUCTION ON", fixture_frames),
        (3, true, "STEREO: LEFT TONE THEN RIGHT TONE", RATE * 3),
    ] {
        handle.try_send(EngineCommand::SetBypass {
            route_id: route_id.clone(),
            bypassed,
        })?;
        wait_for(&handle, &route_id, &cancelled, |route| {
            route.state == PipelineState::Running && route.bypassed == bypassed
        })?;
        // The source is silent while the worker completes the mode transition.
        thread::sleep(Duration::from_millis(500));
        played.store(0, Ordering::Release);
        println!("PHASE {number}: {label}");
        phase.store(number, Ordering::Release);
        let started = Instant::now();
        let mut next_log = Duration::ZERO;
        let mut max_input = 0.0f32;
        let mut max_output = 0.0f32;
        // The metrics retain the last inference timing after returning to bypass.
        let mut inference_measurement_available = false;
        while played.load(Ordering::Acquire) < frames + RATE / 2 {
            ensure_not_cancelled(&cancelled)?;
            if started.elapsed() > Duration::from_secs_f64(frames as f64 / RATE as f64 + 10.0) {
                return Err(io::Error::other("fixture playback stalled").into());
            }
            let route = current_route(&handle, &route_id)?;
            let metrics = &route.metrics;
            max_input = max_input.max(metrics.input_peak);
            max_output = max_output.max(metrics.output_peak);
            inference_measurement_available |= metrics.inference_last_us > 0;
            if started.elapsed() >= next_log {
                println!(
                    "METRICS phase={number} elapsed={:.1} state={:?} bypass={} in={:.5} out={:.5} p95_us={} p99_us={} missed={} overruns={} underruns={} resets={} skipped={} degraded={} latency_ms={:.1}",
                    started.elapsed().as_secs_f32(),
                    route.state,
                    route.bypassed,
                    metrics.input_peak,
                    metrics.output_peak,
                    metrics.inference_p95_us,
                    metrics.inference_p99_us,
                    metrics.missed_deadlines,
                    metrics.input_overruns,
                    metrics.output_underruns,
                    metrics.model_resets,
                    metrics.skipped_inferences,
                    metrics.degraded,
                    metrics.estimated_latency_ms
                );
                next_log += Duration::from_secs(2);
            }
            thread::sleep(Duration::from_millis(100));
        }
        phase.store(0, Ordering::Release);
        let route = current_route(&handle, &route_id)?;
        println!(
            "PHASE_SUMMARY phase={number} max_input={max_input:.5} max_output={max_output:.5} inference_measurement_available={inference_measurement_available} snapshot={route:?}"
        );
        if max_input <= 0.001
            || max_output <= 0.001
            || (!bypassed && !inference_measurement_available)
        {
            return Err(io::Error::other("expected signal or inference was not observed").into());
        }
        if route.model_errors != 0
            || route.input_stream.terminal_issues != 0
            || route.output_stream.terminal_issues != 0
        {
            return Err(io::Error::other("route reported model or terminal stream errors").into());
        }
        summaries.push(route);
        thread::sleep(Duration::from_millis(500));
    }
    stream.pause()?;
    println!(
        "FIXTURE_STREAM callbacks={:?} issues={:?}",
        stream.callback_metrics(),
        stream.issue_snapshot()
    );
    drop(stream);
    handle.try_send(EngineCommand::StopRoute(route_id.clone()))?;
    wait_for(&handle, &route_id, &cancelled, |route| {
        route.state == PipelineState::Stopped
    })?;
    engine.shutdown()?;
    let final_inventory = host.inventory()?;
    let defaults_unchanged = inventory
        .devices
        .iter()
        .filter(|device| device.is_default_output || device.is_default_input)
        .all(|before| {
            final_inventory.devices.iter().any(|after| {
                before.id == after.id
                    && before.is_default_input == after.is_default_input
                    && before.is_default_output == after.is_default_output
            })
        });
    println!(
        "RESULT: signal observed in all {} phases; bypass switched both ways; route stopped and engine joined; system_defaults_unchanged={defaults_unchanged}",
        summaries.len()
    );
    println!(
        "LIMITS: listener must confirm audibility, stereo order and speech quality; no long soak, physical microphone, device removal or concurrent-route test was performed."
    );
    Ok(())
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), io::Error> {
    if cancelled.load(Ordering::Acquire) {
        Err(io::Error::other("listening check cancelled"))
    } else {
        Ok(())
    }
}

fn current_route(handle: &AudioEngineHandle, id: &RouteId) -> Result<RouteSnapshot, io::Error> {
    let snapshot = handle.snapshot();
    let route = snapshot
        .routes
        .into_iter()
        .find(|route| &route.config.id == id)
        .ok_or_else(|| {
            io::Error::other(format!(
                "route not published; fault={:?}",
                snapshot.last_fault
            ))
        })?;
    if route.state == PipelineState::Failed {
        return Err(io::Error::other(format!("route failed: {:?}", route.fault)));
    }
    Ok(route)
}

fn wait_for(
    handle: &AudioEngineHandle,
    id: &RouteId,
    cancelled: &AtomicBool,
    ready: impl Fn(&RouteSnapshot) -> bool,
) -> Result<RouteSnapshot, io::Error> {
    let started = Instant::now();
    loop {
        ensure_not_cancelled(cancelled)?;
        let snapshot = handle.snapshot();
        if let Some(fault) = snapshot.last_fault {
            return Err(io::Error::other(format!("engine fault: {fault:?}")));
        }
        if let Some(route) = snapshot
            .routes
            .into_iter()
            .find(|route| &route.config.id == id)
        {
            if route.state == PipelineState::Failed {
                return Err(io::Error::other(format!("route failed: {:?}", route.fault)));
            }
            if ready(&route) {
                return Ok(route);
            }
        }
        if started.elapsed() > Duration::from_secs(45) {
            return Err(io::Error::other("timed out waiting for engine route state"));
        }
        thread::sleep(Duration::from_millis(50));
    }
}
