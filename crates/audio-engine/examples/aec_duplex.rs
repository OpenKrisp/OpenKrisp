//! Explicit opt-in hardware harness. Does not enable AEC in the desktop application.
use audio_cpal::DeviceSelection;
use audio_engine::{
    AecDiagnostics, AecPrototypeConfig, HeadlessMonitor, HeadlessMonitorConfig, NativeAudioTaps,
};
use audio_protocol::{DeviceId, RouteId};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        eprintln!(
            "Usage: aec_duplex INCOMING_CAPTURE_ID PHYSICAL_RENDER_ID MICROPHONE_ID MICROPHONE_OUTPUT_ID\nRuns for 30 seconds. Use distinct virtual buses for incoming capture and microphone output.\nKeep headphones as fallback until open-speaker acceptance passes. Device IDs: openkrisp-monitor --list-devices."
        );
        return Err("four explicit device IDs are required".into());
    }
    if args[0] == args[1] || args[0] == args[3] || args[1] == args[3] || args[2] == args[3] {
        return Err(
            "capture/render buses must not feed themselves or recapture microphone output".into(),
        );
    }
    let select = |value: &str| DeviceId::new(value).map(DeviceSelection::Id);
    let taps = NativeAudioTaps::default();
    let reference = RouteId::new("aec-incoming")?;
    let diagnostics = AecDiagnostics::default();
    let pressure = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Release))?;
    let mut incoming = HeadlessMonitor::start_with_cancellation(
        HeadlessMonitorConfig {
            route_id: reference.clone(),
            audio_taps: taps.clone(),
            incoming: true,
            input: select(&args[0])?,
            output: select(&args[1])?,
            priority_pressure: pressure.clone(),
            ..HeadlessMonitorConfig::default()
        },
        cancelled.clone(),
    )?;
    let mut microphone = HeadlessMonitor::start_with_cancellation(
        HeadlessMonitorConfig {
            route_id: RouteId::new("aec-microphone")?,
            audio_taps: taps,
            input: select(&args[2])?,
            output: select(&args[3])?,
            aec: Some(AecPrototypeConfig {
                reference_route_id: reference,
                diagnostics: diagnostics.clone(),
            }),
            ..HeadlessMonitorConfig::default()
        },
        cancelled.clone(),
    )?;
    let start = Instant::now();
    let mut last_report = start;
    let mut missed = 0;
    while start.elapsed() < Duration::from_secs(30) && !cancelled.load(Ordering::Acquire) {
        microphone.poll_health();
        incoming.poll_health();
        let metrics = microphone.snapshot().metrics;
        pressure.store(
            metrics.degraded || metrics.missed_deadlines > missed,
            Ordering::Release,
        );
        missed = metrics.missed_deadlines;
        if last_report.elapsed() >= Duration::from_secs(1) {
            println!("AEC={:?}; microphone={metrics:?}", diagnostics.snapshot());
            last_report = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    microphone.stop()?;
    incoming.stop()?;
    Ok(())
}
