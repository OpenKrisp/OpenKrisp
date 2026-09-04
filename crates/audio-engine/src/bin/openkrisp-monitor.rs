use std::{
    error::Error,
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use audio_cpal::{CpalHost, DeviceSelection};
use audio_engine::{HeadlessMonitor, HeadlessMonitorConfig};
use audio_protocol::{DeviceId, PipelineState};

#[derive(Debug, Default)]
struct CliOptions {
    list_devices: bool,
    help: bool,
    input: Option<String>,
    output: Option<String>,
    model: Option<PathBuf>,
    latency_ms: Option<u16>,
    duration_seconds: Option<u64>,
    attenuation_limit_db: Option<f32>,
    bypassed: bool,
    post_filter_enabled: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options(std::env::args().skip(1))?;
    if options.help {
        print_help();
        return Ok(());
    }
    if options.list_devices {
        print_devices()?;
        return Ok(());
    }
    run_monitor(options)
}

fn run_monitor(options: CliOptions) -> Result<(), Box<dyn Error>> {
    let running = Arc::new(AtomicBool::new(true));
    let signal_running = running.clone();
    ctrlc::set_handler(move || signal_running.store(false, Ordering::Release))?;

    let mut config = HeadlessMonitorConfig {
        input: parse_device_selection(options.input)?,
        output: parse_device_selection(options.output)?,
        ..HeadlessMonitorConfig::default()
    };
    if let Some(model) = options.model {
        config.model_path = model;
    }
    if let Some(latency_ms) = options.latency_ms {
        config.target_latency_ms = latency_ms;
    }
    config.initially_bypassed = options.bypassed;
    config.denoise.attenuation_limit_db = options.attenuation_limit_db;
    config.denoise.post_filter_enabled = options.post_filter_enabled;

    println!("Starting OpenKrisp headless monitor. Use headphones to avoid feedback.");
    let mut monitor = HeadlessMonitor::start(config)?;
    let initial = monitor.snapshot();
    println!(
        "Input:  {} ({})",
        initial.input_device.name, initial.input_device.id
    );
    println!(
        "Output: {} ({})",
        initial.output_device.name, initial.output_device.id
    );
    println!("Input format:  {:?}", initial.input_format);
    println!("Output format: {:?}", initial.output_format);
    println!(
        "Model: {} ({} Hz, {} samples/frame)",
        initial.model.model_id,
        initial.model.sample_rate_hz.get(),
        initial.model.frame_samples_per_channel
    );

    let started = Instant::now();
    while running.load(Ordering::Acquire)
        && options
            .duration_seconds
            .is_none_or(|seconds| started.elapsed() < Duration::from_secs(seconds))
    {
        thread::sleep(Duration::from_secs(1));
        let snapshot = monitor.snapshot();
        let metrics = snapshot.metrics;
        println!(
            "state={:?} in={:.3}/{:.3} out={:.3}/{:.3} inference={}us p95={}us p99={}us queues(in/out)={}/{} watermarks(in-high/out-low)={}/{} overruns={} underruns={} discontinuities={} missed={} model_errors={} resets={} issues(in/out)={}/{} terminal(in/out)={}/{} xruns={} violations={} callbacks(in/out)={}/{} latency={:.1}ms",
            snapshot.state,
            metrics.input_peak,
            metrics.input_rms,
            metrics.output_peak,
            metrics.output_rms,
            metrics.inference_last_us,
            metrics.inference_p95_us,
            metrics.inference_p99_us,
            metrics.input_ring_fill_frames,
            metrics.output_ring_fill_frames,
            metrics.input_ring_high_water_frames,
            metrics.output_ring_low_water_frames,
            metrics.input_overruns,
            metrics.output_underruns,
            metrics.discontinuities,
            metrics.missed_deadlines,
            snapshot.model_errors,
            metrics.model_resets,
            snapshot.input_issues.total,
            snapshot.output_issues.total,
            snapshot.input_issues.terminal,
            snapshot.output_issues.terminal,
            snapshot.input_issues.xruns + snapshot.output_issues.xruns,
            snapshot.input_issues.callback_violations + snapshot.output_issues.callback_violations,
            snapshot.input_callbacks.callbacks,
            snapshot.output_callbacks.callbacks,
            metrics.estimated_latency_ms,
        );
        if snapshot.state == PipelineState::Failed {
            eprintln!(
                "route failure: last input issue={:?}, last output issue={:?}",
                snapshot.input_issues.last_kind, snapshot.output_issues.last_kind
            );
            monitor.stop()?;
            return Err(
                io::Error::other("audio route failed; inspect stream counters above").into(),
            );
        }
    }
    monitor.stop()?;
    println!("OpenKrisp monitor stopped cleanly.");
    Ok(())
}

fn print_devices() -> Result<(), Box<dyn Error>> {
    let inventory = CpalHost::new_default().inventory()?;
    println!("CPAL host: {}", inventory.host);
    if inventory.devices.is_empty() {
        println!("No audio devices found.");
        return Ok(());
    }
    for device in inventory.devices {
        let mut roles = Vec::new();
        if device.supports_input {
            roles.push("input");
        }
        if device.supports_output {
            roles.push("output");
        }
        if device.is_default_input {
            roles.push("default-input");
        }
        if device.is_default_output {
            roles.push("default-output");
        }
        println!(
            "{}\n  id: {}\n  roles: {}",
            device.name,
            device.id,
            roles.join(", ")
        );
        if let Some(format) = device.default_input_format {
            println!("  default input:  {format:?}");
        }
        if let Some(format) = device.default_output_format {
            println!("  default output: {format:?}");
        }
    }
    Ok(())
}

fn parse_device_selection(value: Option<String>) -> Result<DeviceSelection, Box<dyn Error>> {
    match value.as_deref() {
        None | Some("default") => Ok(DeviceSelection::Default),
        Some(value) => Ok(DeviceSelection::Id(DeviceId::new(value)?)),
    }
}

fn parse_options(
    arguments: impl IntoIterator<Item = String>,
) -> Result<CliOptions, Box<dyn Error>> {
    let mut options = CliOptions {
        post_filter_enabled: true,
        ..CliOptions::default()
    };
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--list-devices" => options.list_devices = true,
            "--help" | "-h" => options.help = true,
            "--bypass" => options.bypassed = true,
            "--no-post-filter" => options.post_filter_enabled = false,
            "--input" => options.input = Some(next_value(&mut arguments, "--input")?),
            "--output" => options.output = Some(next_value(&mut arguments, "--output")?),
            "--model" => {
                options.model = Some(PathBuf::from(next_value(&mut arguments, "--model")?));
            }
            "--latency-ms" => {
                options.latency_ms = Some(parse_value(
                    &next_value(&mut arguments, "--latency-ms")?,
                    "--latency-ms",
                )?);
            }
            "--duration-seconds" => {
                options.duration_seconds = Some(parse_value(
                    &next_value(&mut arguments, "--duration-seconds")?,
                    "--duration-seconds",
                )?);
            }
            "--attenuation-limit-db" => {
                options.attenuation_limit_db = Some(parse_value(
                    &next_value(&mut arguments, "--attenuation-limit-db")?,
                    "--attenuation-limit-db",
                )?);
            }
            _ => return Err(invalid_input(format!("unknown argument: {argument}"))),
        }
    }
    Ok(options)
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn Error>> {
    arguments
        .next()
        .ok_or_else(|| invalid_input(format!("{flag} requires a value")))
}

fn parse_value<T>(value: &str, flag: &str) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|error| invalid_input(format!("invalid value for {flag}: {error}")))
}

fn invalid_input(message: String) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidInput, message).into()
}

fn print_help() {
    println!(
        "OpenKrisp headless microphone monitor\n\n\
Usage:\n  openkrisp-monitor --list-devices\n  openkrisp-monitor [options]\n\n\
Options:\n  --input ID                    CPAL input device ID (default: current default)\n  --output ID                   CPAL output device ID (default: current default)\n  --model PATH                  Model archive with adjacent manifest.json\n  --latency-ms N                Startup output prefill in milliseconds (default: 40)\n  --duration-seconds N          Stop automatically after N seconds\n  --attenuation-limit-db DB     DeepFilterNet attenuation limit\n  --no-post-filter              Disable the verified post-filter control\n  --bypass                      Start on the latency-aligned dry path\n  --list-devices                Print stable device IDs and formats\n  -h, --help                    Print this help"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_route_flags_without_hardware() {
        let options = parse_options([
            "--input".to_owned(),
            "coreaudio:input".to_owned(),
            "--output".to_owned(),
            "default".to_owned(),
            "--latency-ms".to_owned(),
            "60".to_owned(),
            "--duration-seconds".to_owned(),
            "2".to_owned(),
            "--attenuation-limit-db".to_owned(),
            "24".to_owned(),
            "--bypass".to_owned(),
            "--no-post-filter".to_owned(),
        ])
        .expect("valid CLI");
        assert_eq!(options.input.as_deref(), Some("coreaudio:input"));
        assert_eq!(options.latency_ms, Some(60));
        assert_eq!(options.duration_seconds, Some(2));
        assert_eq!(options.attenuation_limit_db, Some(24.0));
        assert!(options.bypassed);
        assert!(!options.post_filter_enabled);
    }

    #[test]
    fn rejects_missing_and_unknown_arguments() {
        assert!(parse_options(["--input".to_owned()]).is_err());
        assert!(parse_options(["--unknown".to_owned()]).is_err());
    }
}
