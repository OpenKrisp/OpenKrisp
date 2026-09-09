//! Fixed PipeWire loopback client. The route owner creates/drops it off the audio callbacks.
//! No shell, default-device changes, persistent configuration, or audio over process pipes.

use crate::RoutingError;
use std::process::{Child, Command, Stdio};

pub const LOOPBACK_ARGS: &[&str] = &[
    "--name=openkrisp",
    "--channels=2",
    "--capture-props=media.class=Audio/Sink node.name=openkrisp_sink node.description=\"OpenKrisp Input\" node.virtual=true node.autoconnect=false audio.rate=48000",
    "--playback-props=media.class=Audio/Source node.name=openkrisp_source node.description=\"OpenKrisp Microphone\" node.virtual=true node.autoconnect=false audio.rate=48000",
];

/// Removing our client removes only its nodes and links. Existing external endpoints are not owned.
pub struct PipeWireSession {
    child: Child,
}

impl PipeWireSession {
    pub fn start() -> Result<Self, RoutingError> {
        Self::spawn(Command::new("pw-loopback").args(LOOPBACK_ARGS))
    }

    pub fn start_incoming() -> Result<Self, RoutingError> {
        Self::spawn(Command::new("pw-loopback").args([
            "--name=openkrisp-incoming",
            "--channels=2",
            "--capture-props=media.class=Audio/Sink node.name=openkrisp_incoming_sink node.description=\"OpenKrisp Incoming Audio\" node.virtual=true node.autoconnect=false audio.rate=48000",
            "--playback-props=media.class=Audio/Source node.name=openkrisp_incoming_source node.description=\"OpenKrisp Incoming Capture\" node.virtual=true node.autoconnect=false audio.rate=48000",
        ]))
    }

    fn spawn(command: &mut Command) -> Result<Self, RoutingError> {
        #[cfg(target_os = "linux")]
        crate::linux_process::bind_to_parent(command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map(|child| Self { child })
            .map_err(|_| RoutingError::EndpointCreationFailed)
    }

    pub fn check_running(&mut self) -> Result<(), RoutingError> {
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            _ => Err(RoutingError::EndpointCreationFailed),
        }
    }
}

impl Drop for PipeWireSession {
    fn drop(&mut self) {
        // kill addresses the owned child handle/PID; never kill a process found by name.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn process_creation_and_early_exit_are_actionable() {
        assert!(matches!(
            PipeWireSession::spawn(&mut Command::new("/nonexistent/openkrisp-pw-loopback")),
            Err(RoutingError::EndpointCreationFailed)
        ));
        let mut session =
            PipeWireSession::spawn(&mut Command::new("/usr/bin/false")).expect("spawn");
        let deadline = Instant::now() + Duration::from_secs(2);
        while session.check_running().is_ok() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(
            session.check_running(),
            Err(RoutingError::EndpointCreationFailed)
        );
    }

    #[test]
    fn drop_reaps_owned_process() {
        let session = PipeWireSession::spawn(Command::new("/bin/sleep").arg("30")).expect("spawn");
        let pid = session.child.id();
        drop(session);
        #[cfg(target_os = "linux")]
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "owned child must be reaped"
        );
        #[cfg(not(target_os = "linux"))]
        {
            let status = Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .expect("check process");
            assert!(!status.success(), "owned child must be gone after teardown");
        }
    }

    // Invoked in a separate test process so termination really bypasses Rust destructors.
    #[cfg(target_os = "linux")]
    #[test]
    fn parent_death_helper() {
        let Ok(ready_file) = std::env::var("OPENKRISP_PARENT_DEATH_READY") else {
            return;
        };
        let session = PipeWireSession::spawn(Command::new("/bin/sleep").arg("30"))
            .expect("spawn owned client");
        std::fs::write(ready_file, session.child.id().to_string()).expect("announce child");
        if std::env::var("OPENKRISP_PARENT_DEATH_MODE").as_deref() == Ok("exit") {
            std::process::exit(17);
        }
        loop {
            std::thread::park();
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn client_dies_when_owner_exits_without_drop_or_receives_term_or_kill() {
        for mode in ["exit", "term", "kill"] {
            let temporary = tempfile::tempdir().expect("test directory");
            let ready_file = temporary.path().join("child-pid");
            // This guard cleans up the helper even if a readiness assertion fails.
            let mut owner = PipeWireSession {
                child: Command::new(std::env::current_exe().expect("test executable"))
                    .args([
                        "--exact",
                        "pipewire::tests::parent_death_helper",
                        "--nocapture",
                    ])
                    .env("OPENKRISP_PARENT_DEATH_READY", &ready_file)
                    .env("OPENKRISP_PARENT_DEATH_MODE", mode)
                    .stdout(Stdio::null())
                    .spawn()
                    .expect("spawn owner"),
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            let pid: u32 = loop {
                if let Ok(text) = std::fs::read_to_string(&ready_file)
                    && let Ok(pid) = text.parse()
                {
                    break pid;
                }
                assert!(Instant::now() < deadline, "owner failed to announce child");
                std::thread::sleep(Duration::from_millis(10));
            };
            match mode {
                "term" => assert!(
                    Command::new("/bin/sh")
                        .args([
                            "-c",
                            "kill -TERM \"$1\"",
                            "openkrisp-test",
                            &owner.child.id().to_string()
                        ])
                        .status()
                        .expect("terminate owner")
                        .success()
                ),
                "kill" => owner.child.kill().expect("kill owner"),
                _ => (),
            }
            owner.child.wait().expect("reap owner");
            let running = || {
                // Container PID 1 may defer reaping an orphan. Zombies hold no audio resources.
                std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .and_then(|stat| {
                        stat.rsplit_once(") ")
                            .map(|(_, fields)| fields.starts_with('Z'))
                    })
                    .is_some_and(|zombie| !zombie)
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            while running() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let survived = running();
            if survived {
                // Also clean up when testing a regression that would otherwise leak the fixture.
                let _ = Command::new("/bin/sh")
                    .args([
                        "-c",
                        "kill -KILL \"$1\"",
                        "openkrisp-test",
                        &pid.to_string(),
                    ])
                    .status();
            }
            assert!(!survived, "client survived owner termination ({mode})");
        }
    }
}
