//! Real DSP threads and production models with synthetic callbacks. No native device opens.
use audio_engine::{
    PipelineWorkerConfig, RouteMetricsStore, bounded_audio_ring, spawn_pipeline_worker,
};
use audio_protocol::{AudioFormat, ChannelCount, DenoiseSettings, SampleFormat, SampleRateHz};
use denoise_core::{IndependentChannels, NoiseReducer};
use denoise_dfn::{DfnNoiseReducer, ModelAsset, default_model_path};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

fn rss_kib() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status")
            .ok()?
            .lines()
            .find(|line| line.starts_with("VmRSS:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }
    #[cfg(target_os = "macos")]
    {
        let result = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(result.stdout).ok()?.trim().parse().ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[test]
#[ignore = "two-hour concurrent production-model soak: run explicitly with --release"]
#[allow(clippy::assertions_on_constants)]
fn concurrent_microphone_and_stereo_incoming_soak() {
    assert!(!cfg!(debug_assertions), "run soak with --release");
    let seconds = std::env::var("OPENKRISP_SOAK_SECONDS")
        .map_or(7200, |x| x.parse::<u64>().expect("positive seconds"));
    assert!((1..=7200).contains(&seconds));
    let pressure = Arc::new(AtomicBool::new(false));
    let asset = ModelAsset::load(default_model_path()).unwrap();
    let mut routes = Vec::new();
    for channels in [1, 2] {
        let format = AudioFormat {
            sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
            channels: ChannelCount::new(channels).unwrap(),
            sample_format: SampleFormat::F32,
            buffer_size_frames: None,
        };
        let (input, consumer) = bounded_audio_ring(4800, format.channels).unwrap();
        let (mut output, sink) = bounded_audio_ring(9600, format.channels).unwrap();
        output.write(&vec![0.; 1920 * channels as usize]).unwrap();
        let metrics = RouteMetricsStore::new(
            input.metrics(),
            output.metrics(),
            format.clone(),
            format.clone(),
        );
        let mut config = PipelineWorkerConfig::new(format.clone(), format.clone(), 3840);
        config.incoming = channels == 2;
        config.priority_pressure = pressure.clone();
        let asset = asset.clone();
        let worker = spawn_pipeline_worker(config, consumer, output, metrics.clone(), move || {
            let mut reducer = IndependentChannels::new(format.channels, || {
                DfnNoiseReducer::from_asset(asset.clone(), DenoiseSettings::default())
            })?;
            let input = vec![0.001; reducer.descriptor().frame_samples()?];
            let mut output = input.clone();
            for _ in 0..3 {
                reducer.process_planar(&input, &mut output)?;
            }
            reducer.reset()?;
            Ok(reducer)
        })
        .unwrap();
        routes.push((
            input,
            sink,
            worker,
            metrics,
            vec![0.; 480 * channels as usize],
            vec![0.; 480 * channels as usize],
        ));
    }
    let started = Instant::now();
    let mut baseline_rss = None;
    let mut peak_rss = 0;
    let mut max_latency = 0.0_f32;
    for tick in 0..seconds * 100 {
        let deadline = started + Duration::from_millis(tick * 10);
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        // Pressure release and bypass changes exercise temporal reset while both workers live.
        pressure.store(tick % 3000 >= 2000, Ordering::Release);
        if tick % 1500 == 0 {
            for (_, _, worker, _, _, _) in &routes {
                worker.set_bypass(tick % 3000 == 1500).unwrap();
            }
        }
        for (route_index, (input, sink, worker, metrics, captured, rendered)) in
            routes.iter_mut().enumerate()
        {
            for (i, sample) in captured.iter_mut().enumerate() {
                *sample = 0.07
                    * ((tick * 480 + (i / (route_index + 1)) as u64) as f32
                        * (0.03 + route_index as f32 * 0.017))
                        .sin();
            }
            input.write(captured).unwrap();
            worker.wake();
            sink.read_or_silence(rendered).unwrap();
            assert!(rendered.iter().all(|x| x.is_finite() && x.abs() <= 0.98));
            assert!(!worker.is_finished(), "worker exited during soak");
            let snapshot = metrics.snapshot();
            max_latency = max_latency.max(snapshot.estimated_latency_ms);
            assert!(snapshot.estimated_latency_ms < 200.);
            assert_eq!(metrics.model_errors(), 0);
        }
        if tick % 100 == 0 {
            let rss = rss_kib();
            if std::env::var_os("OPENKRISP_REQUIRE_RSS").is_some() {
                assert!(rss.is_some(), "RSS measurement unavailable");
            }
            if (tick >= 500 || seconds < 5)
                && let Some(rss) = rss
            {
                baseline_rss.get_or_insert(rss);
                peak_rss = peak_rss.max(rss);
                assert!(
                    rss <= baseline_rss.unwrap() + 65536,
                    "RSS grew by more than 64 MiB after startup"
                );
            }
            if tick % 6000 == 0 {
                eprintln!(
                    "OPENKRISP_SOAK {}",
                    serde_json::json!({"second":tick/100,"rssKiB":rss,"microphone":routes[0].3.snapshot(),"incoming":routes[1].3.snapshot()})
                );
            }
        }
    }
    for (_, _, worker, metrics, _, _) in &mut routes {
        worker.shutdown().unwrap();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.input_overruns, 0);
        assert!(
            snapshot.output_underruns <= 5 + seconds / 100,
            "sustained underruns: {snapshot:?}"
        );
        assert!(!snapshot.degraded);
    }
    eprintln!(
        "OPENKRISP_SOAK_RESULT {}",
        serde_json::json!({"seconds":seconds,"baselineRssKiB":baseline_rss,"peakRssKiB":peak_rss,"maxLatencyMs":max_latency,"joinedWorkers":routes.len(),"microphone":routes[0].3.snapshot(),"incoming":routes[1].3.snapshot()})
    );
}
