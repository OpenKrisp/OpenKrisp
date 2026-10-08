use super::*;
use crate::{
    config_store::ConfigStore, settings_writer::SaveSettingsRequest, state::EngineControl,
};
use audio_engine::{EngineCommand, EngineError};
use audio_protocol::EngineSnapshot;
use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
};

struct IdleEngine;
impl EngineControl for IdleEngine {
    fn try_send(&self, _: EngineCommand) -> Result<(), EngineError> {
        Ok(())
    }
    fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot::default()
    }
}
fn app_state(path: &std::path::Path) -> AppState {
    AppState::for_test(Arc::new(IdleEngine), ConfigStore::open(path.to_owned()))
}
struct Installer {
    attempts: Arc<Mutex<Vec<Vec<u8>>>>,
    failures: Mutex<usize>,
}
impl UpdateInstaller for Installer {
    fn install(&self, bytes: &[u8]) -> Result<(), String> {
        self.attempts.lock().unwrap().push(bytes.to_vec());
        let mut failures = self.failures.lock().unwrap();
        if *failures > 0 {
            *failures -= 1;
            Err("administrator cancelled".into())
        } else {
            Ok(())
        }
    }
}
fn pending(failures: usize, attempts: Arc<Mutex<Vec<Vec<u8>>>>) -> DownloadedUpdate {
    DownloadedUpdate {
        installer: Box::new(Installer {
            failures: Mutex::new(failures),
            attempts,
        }),
        bytes: b"verified fixture".to_vec(),
    }
}

#[test]
fn enabled_downloads_report_progress_coalesce_and_retry_failures() {
    tauri::async_runtime::block_on(async {
        let state = UpdateState::new(true);
        let first = prepare_with(&state, async {
            state.change(|session| {
                session.status.phase = UpdatePhase::Downloading;
                session.status.version = Some("0.2.0".into());
                session.status.downloaded_bytes = 4;
                session.status.total_bytes = Some(16);
            });
            let overlapping = prepare_with(&state, async {
                panic!("overlapping check reached transport")
            })
            .await;
            assert_eq!(overlapping.phase, UpdatePhase::Downloading);
            assert_eq!(overlapping.downloaded_bytes, 4);
            Err("HTTP 503".into())
        })
        .await;
        assert_eq!(first.phase, UpdatePhase::Error);
        assert!(state.lock().pending.is_none());
        let next = prepare_with(&state, async { Ok(Some(pending(0, Arc::default()))) }).await;
        assert_eq!(next.phase, UpdatePhase::Ready);
        assert!(next.revision > first.revision);
        assert!(next.error.is_none());
        assert_eq!(next.downloaded_bytes, 0);
    });
}

#[test]
fn failed_install_retains_verified_bytes_and_success_flushes_settings_before_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let app = app_state(&path);
    let updates = UpdateState::new(true);
    let attempts = Arc::default();
    tauri::async_runtime::block_on(prepare_with(&updates, async {
        Ok(Some(pending(1, Arc::clone(&attempts))))
    }));
    assert!(install_prepared(&app, &updates).is_err());
    assert_eq!(updates.lock().status.phase, UpdatePhase::Ready);
    assert!(updates.lock().pending.is_some());
    assert!(!app.shutdown_started.load(Ordering::Acquire));
    let mut settings = app.settings_writer.snapshot().settings;
    settings.incoming.output_id = "saved-before-restart".into();
    app.settings_writer
        .submit(SaveSettingsRequest {
            settings: settings.clone(),
            revision: 1,
            replace_unreadable: false,
        })
        .unwrap();
    install_prepared(&app, &updates).unwrap();
    assert!(app.shutdown_finished.load(Ordering::Acquire));
    assert_eq!(ConfigStore::open(path).initial, settings);
    assert_eq!(
        *attempts.lock().unwrap(),
        vec![b"verified fixture".to_vec(); 2]
    );
    assert!(updates.lock().pending.is_none());
}

#[test]
fn shutdown_revokes_a_download_that_finishes_late() {
    tauri::async_runtime::block_on(async {
        let updates = UpdateState::new(true);
        let status = prepare_with(&updates, async {
            updates.stop_scheduler();
            Ok(Some(pending(0, Arc::default())))
        })
        .await;
        assert_ne!(status.phase, UpdatePhase::Ready);
        assert!(updates.lock().pending.is_none());
        prepare_with(&updates, async {
            panic!("closed updater reached transport")
        })
        .await;
    });
}

struct BlockingInstaller {
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl UpdateInstaller for BlockingInstaller {
    fn install(&self, _: &[u8]) -> Result<(), String> {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        Err("cancelled".into())
    }
}
#[test]
fn concurrent_install_and_quit_cannot_race_an_active_installer() {
    let directory = tempfile::tempdir().unwrap();
    let app = Arc::new(app_state(&directory.path().join("settings.json")));
    let updates = Arc::new(UpdateState::new(true));
    let (entered, wait) = mpsc::sync_channel(1);
    let (release, receiver) = mpsc::sync_channel(1);
    tauri::async_runtime::block_on(prepare_with(&updates, async {
        Ok(Some(DownloadedUpdate {
            installer: Box::new(BlockingInstaller {
                entered,
                release: Mutex::new(receiver),
            }),
            bytes: vec![1],
        }))
    }));
    let worker = {
        let app = app.clone();
        let updates = updates.clone();
        thread::spawn(move || install_prepared(&app, &updates))
    };
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(install_prepared(&app, &updates).is_err());
    assert!(!app.try_begin_shutdown());
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    assert!(app.try_begin_shutdown());
    app.shutdown().unwrap();
}

// Public, non-secret minisign-verify 0.2.5 test vector (MIT), payload b"test".
const TEST_PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

// Exercise the pinned plugin's real HTTP download and cryptographic verification.
// Never call its platform installer; only the private fixture installer runs above.
#[test]
#[cfg(debug_assertions)]
fn plugin_rejects_tampered_download_before_it_can_become_ready() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::Instant,
    };
    for bytes in [b"test", b"Test"] {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        server.set_nonblocking(true).unwrap();
        let address = server.local_addr().unwrap();
        let signature = STANDARD.encode(TEST_SIGNATURE);
        let manifest = serde_json::to_vec(&serde_json::json!({"version":"99.0.0", "url":format!("http://{address}/artifact"),"signature":signature})).unwrap();
        let responses = [manifest, bytes.to_vec()];
        let server_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            for body in responses {
                let mut connection = loop {
                    if let Ok((connection, _)) = server.accept() {
                        break connection;
                    }
                    assert!(Instant::now() < deadline, "plugin did not request fixture");
                    thread::sleep(Duration::from_millis(1));
                };
                connection
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 4096];
                let mut received = 0;
                loop {
                    let count = connection.read(&mut request[received..]).unwrap();
                    assert!(count > 0, "fixture request ended before headers");
                    received += count;
                    if request[..received].windows(4).any(|x| x == b"\r\n\r\n") {
                        break;
                    }
                    assert!(
                        received < request.len(),
                        "fixture request exceeds header bound"
                    );
                }
                write!(connection, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                connection.write_all(&body).unwrap();
            }
        });
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert("updater".into(), serde_json::json!({"dangerousInsecureTransportProtocol":true, "pubkey":STANDARD.encode(TEST_PUBLIC_KEY)}));
        let app = tauri::test::mock_builder()
            .plugin(
                tauri_plugin_updater::Builder::new()
                    .pubkey(STANDARD.encode(TEST_PUBLIC_KEY))
                    .build(),
            )
            .build(context)
            .unwrap();
        let updates = UpdateState::new(true);
        let status = tauri::async_runtime::block_on(prepare_with(&updates, async {
            let updater = app
                .updater_builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .endpoints(vec![format!("http://{address}/manifest").parse().unwrap()])
                .unwrap()
                .build()
                .unwrap();
            let update = updater.check().await.map_err(|e| e.to_string())?.unwrap();
            let bytes = update
                .download(
                    |chunk, total| {
                        updates.change(|s| {
                            s.status.phase = UpdatePhase::Downloading;
                            s.status.downloaded_bytes += chunk as u64;
                            s.status.total_bytes = total;
                        });
                    },
                    || {},
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(Some(DownloadedUpdate {
                installer: Box::new(update),
                bytes,
            }))
        }));
        server_thread.join().unwrap();
        if bytes == b"test" {
            assert_eq!(status.phase, UpdatePhase::Ready);
        } else {
            assert_eq!(status.phase, UpdatePhase::Error);
            assert!(updates.lock().pending.is_none());
        }
    }
}
