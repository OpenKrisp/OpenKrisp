//! Opt-in test for an isolated PipeWire session; no physical devices are opened.
#![cfg(target_os = "linux")]

use audio_cpal::{CpalHost, DeviceSelection, NegotiationPreferences, StreamIssueStore};
use audio_protocol::EndpointAvailability;
use platform_routing::{linux::LinuxRouter, pipewire::PipeWireSession};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires an isolated PipeWire + session-manager session; scripts/test-linux-pipewire.sh"]
fn virtual_cable_transfers_pcm_and_tears_down_twice() {
    assert_eq!(std::env::var("OPENKRISP_PIPEWIRE_TEST").as_deref(), Ok("1"));
    let router = LinuxRouter;
    assert_eq!(
        router
            .endpoint(&CpalHost::new_default().inventory().expect("inventory"))
            .availability,
        EndpointAvailability::Missing
    );
    for _ in 0..2 {
        let mut session = PipeWireSession::start().expect("start loopback");
        let endpoint = wait_for(EndpointAvailability::Ready, Some(&mut session));
        session.check_running().expect("loopback running");
        let host = CpalHost::new_default();
        let input = host
            .resolve_input(&DeviceSelection::Id(
                endpoint.capture_device_id.expect("capture"),
            ))
            .expect("input");
        let output = host
            .resolve_output(&DeviceSelection::Id(
                endpoint.playback_device_id.expect("playback"),
            ))
            .expect("output");
        assert!(
            !output.descriptor().supports_input,
            "sink monitor must not be a microphone"
        );
        let received = Arc::new(AtomicUsize::new(0));
        let capture_received = received.clone();
        let input_config = input
            .negotiate_input(NegotiationPreferences::new(
                48_000.try_into().expect("rate"),
                2.try_into().expect("channels"),
            ))
            .expect("input format");
        let output_config = output
            .negotiate_output(NegotiationPreferences::new(
                48_000.try_into().expect("rate"),
                2.try_into().expect("channels"),
            ))
            .expect("output format");
        let capture = input
            .build_input_stream(
                &input_config,
                move |samples| {
                    let matching = samples
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .filter(|frame| {
                            (frame[0] - 0.125).abs() < 0.001 && (frame[1] + 0.25).abs() < 0.001
                        })
                        .count();
                    capture_received.fetch_add(matching, Ordering::Relaxed);
                },
                StreamIssueStore::default(),
            )
            .expect("capture stream");
        let playback = output
            .build_output_stream(
                &output_config,
                |samples| {
                    for frame in samples.as_chunks_mut::<2>().0 {
                        frame.copy_from_slice(&[0.125, -0.25]);
                    }
                },
                StreamIssueStore::default(),
            )
            .expect("playback stream");
        capture.play().expect("start capture");
        playback.play().expect("start playback");
        let deadline = Instant::now() + Duration::from_secs(5);
        while received.load(Ordering::Relaxed) < 1024 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if received.load(Ordering::Relaxed) < 1024 {
            eprintln!(
                "capture callbacks: {:?}, playback callbacks: {:?}",
                capture.callback_metrics(),
                playback.callback_metrics()
            );
            if let Ok(dump) = std::process::Command::new("pw-dump").output() {
                eprintln!("{}", String::from_utf8_lossy(&dump.stdout));
            }
        }
        assert!(
            received.load(Ordering::Relaxed) >= 1024,
            "PCM must arrive at the virtual microphone with both channels intact"
        );
        drop((capture, playback, input, output, host));
        drop(session);
        wait_for(EndpointAvailability::Missing, None);
    }
    // Exercise the real PipeWire client after losing its owner without Drop. A subsequent
    // route must create/own fresh endpoints, not inherit a leaked graph from a dead process.
    for signal in ["-TERM", "-KILL"] {
        let temporary = tempfile::tempdir().expect("helper directory");
        let ready = temporary.path().join("ready");
        let mut owner = OwnerProcess(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "loopback_owner_helper",
                    "--nocapture",
                ])
                .env("OPENKRISP_LOOPBACK_OWNER_READY", &ready)
                .spawn()
                .expect("start owner"),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                owner.0.try_wait().expect("owner status").is_none(),
                "owner exited early"
            );
            assert!(
                Instant::now() < deadline,
                "owner did not start the loopback"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        wait_for(EndpointAvailability::Ready, None);
        assert!(
            std::process::Command::new("/bin/sh")
                .args([
                    "-c",
                    "kill \"$1\" \"$2\"",
                    "openkrisp-test",
                    signal,
                    &owner.0.id().to_string()
                ])
                .status()
                .unwrap()
                .success()
        );
        owner.0.wait().expect("reap owner");
        wait_for(EndpointAvailability::Missing, None);
        let mut restarted = PipeWireSession::start().expect("recreate after crash");
        wait_for(EndpointAvailability::Ready, Some(&mut restarted));
        drop(restarted);
        wait_for(EndpointAvailability::Missing, None);
    }
}

struct OwnerProcess(std::process::Child);

impl Drop for OwnerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "subprocess helper for the isolated PipeWire test"]
fn loopback_owner_helper() {
    let Ok(ready) = std::env::var("OPENKRISP_LOOPBACK_OWNER_READY") else {
        return;
    };
    assert_eq!(std::env::var("OPENKRISP_PIPEWIRE_TEST").as_deref(), Ok("1"));
    let mut session = PipeWireSession::start().expect("start owned loopback");
    session.check_running().expect("loopback alive");
    std::fs::write(ready, "ready").expect("announce readiness");
    loop {
        std::thread::park();
    }
}

fn wait_for(
    expected: EndpointAvailability,
    mut session: Option<&mut PipeWireSession>,
) -> audio_protocol::PlatformEndpoint {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(session) = session.as_mut() {
            session
                .check_running()
                .expect("loopback process exited before discovery");
        }
        let endpoint =
            LinuxRouter.endpoint(&CpalHost::new_default().inventory().expect("inventory"));
        if endpoint.availability == expected {
            return endpoint;
        }
        assert!(
            Instant::now() < deadline,
            "expected {expected:?}, got {endpoint:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
