//! Opt-in native test build. No fixture commands, state, or switches exist in default builds.
use crate::{
    config_store::ConfigStore,
    state::{AppState, EngineControl},
};
use audio_engine::{EngineCommand, EngineError};
use audio_protocol::{EngineSnapshot, PipelineState, RouteSnapshot};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{
    App, AppHandle, Manager, Runtime, Webview,
    webview::{PageLoadEvent, PageLoadPayload},
};

fn options() -> Option<(bool, PathBuf)> {
    let mut args = std::env::args().skip(1);
    let mode = args.next()?;
    if !matches!(mode.as_str(), "--smoke-fixture" | "--smoke-startup") {
        return None;
    }
    let directory = PathBuf::from(args.next().expect("smoke output directory"));
    assert!(
        directory.is_absolute() && directory.is_dir(),
        "use an existing private smoke directory"
    );
    Some((mode == "--smoke-fixture", directory))
}
pub(crate) fn settings_path() -> Option<PathBuf> {
    options().map(|(_, directory)| directory.join("settings.json"))
}
#[derive(Default)]
struct SmokeState {
    pages: AtomicUsize,
    failed: AtomicBool,
    evidence: Mutex<Vec<&'static str>>,
}
struct FixtureEngine(Mutex<EngineSnapshot>);
impl EngineControl for FixtureEngine {
    fn snapshot(&self) -> EngineSnapshot {
        self.0.lock().unwrap().clone()
    }
    fn try_send(&self, command: EngineCommand) -> Result<(), EngineError> {
        let mut snapshot = self.0.lock().unwrap();
        match command {
            EngineCommand::StartRoute(config) => {
                snapshot.routes.retain(|route| route.config.id != config.id);
                snapshot
                    .routes
                    .push(RouteSnapshot::pending(config, PipelineState::Running));
            }
            EngineCommand::StopRoute(id) => {
                if let Some(route) = snapshot
                    .routes
                    .iter_mut()
                    .find(|route| route.config.id == id)
                {
                    route.state = PipelineState::Stopped;
                }
            }
            _ => {}
        }
        snapshot.revision += 1;
        Ok(())
    }
}
pub(crate) fn validate_resource<R: Runtime>(
    app: &App<R>,
    model: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if options().is_some() {
        denoise_dfn::ModelAsset::load(model)?;
        app.manage(SmokeState::default());
        record(app.handle(), "packaged_model_integrity");
    }
    Ok(())
}
pub(crate) fn install_fixture_state<R: Runtime>(app: &App<R>, path: &Path) -> bool {
    if options().is_some_and(|(fixture, _)| fixture) {
        app.manage(AppState::for_test(
            Arc::new(FixtureEngine(Mutex::new(EngineSnapshot::default()))),
            ConfigStore::open(path.to_owned()),
        ));
        true
    } else {
        false
    }
}
fn record<R: Runtime>(app: &AppHandle<R>, evidence: &'static str) {
    app.state::<SmokeState>()
        .evidence
        .lock()
        .unwrap()
        .push(evidence);
}

pub(crate) fn page_loaded<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    let Some((fixture, _)) = options() else {
        return;
    };
    if payload.event() != PageLoadEvent::Finished {
        return;
    }
    let page = webview
        .app_handle()
        .state::<SmokeState>()
        .pages
        .fetch_add(1, Ordering::AcqRel);
    // Calls cross the actual JavaScript -> Tauri IPC boundary in the real native WebView.
    let starts = if fixture && page == 0 {
        start_script()
    } else {
        String::new()
    };
    let revision = page + 1;
    let script = format!(
        r#"(async () => {{
        const invoke = window.__TAURI_INTERNALS__.invoke;
        {starts}
        const snapshot = await invoke('get_engine_snapshot');
        if ({fixture} && snapshot.routes.filter(r => r.state === 'running').length !== 2) throw Error('routes not preserved');
        window.dispatchEvent(new Event('focus'));
        const deadline = Date.now() + 10000;
        while (true) {{
            const microphone = document.querySelector('[aria-labelledby="microphone-title"] .status');
            const incoming = document.querySelector('[aria-labelledby="speakers-title"] .status');
            const rendered = {fixture} ? microphone?.textContent.trim() === 'On' && incoming?.textContent.trim() === 'On'
                : microphone && microphone.textContent.trim() !== 'Reconnecting…';
            if (rendered) break;
            if (Date.now() > deadline) throw Error('frontend did not reconcile native state');
            await new Promise(resolve => setTimeout(resolve, 25));
        }}
        const settings = await invoke('get_settings');
        settings.settings.microphone.inputId = 'smoke:input';
        await invoke('save_settings', {{request:{{settings:settings.settings, revision:{revision}, replaceUnreadable:false}}}});
    }})();"#
    );
    webview
        .eval(&script)
        .expect("native webview evaluates smoke script");
}
fn start_script() -> String {
    ["smoke-microphone", "smoke-incoming"].into_iter().enumerate().map(|(index, id)| {
        format!(r#"await invoke('start_route', {{request:{{routeId:'{id}',sourceDeviceId:'smoke:input',sourceKind:'{}',destinationDeviceId:'smoke:output',destinationKind:'physical_output',mode:'{}',targetLatencyMs:40,denoise:{{enabled:true,attenuationLimitDb:null,postFilterEnabled:false}},echoCancellation:false}}}});"#,
            if index == 0 { "physical_input" } else { "system_loopback" }, if index == 0 { "speech_denoise" } else { "aligned_bypass" })
    }).collect()
}
fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "native smoke phase timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
fn on_main<R: Runtime>(app: &AppHandle<R>, action: impl FnOnce(&AppHandle<R>) + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        action(&handle);
        tx.send(()).unwrap();
    })
    .unwrap();
    rx.recv_timeout(Duration::from_secs(15)).unwrap();
}
pub(crate) fn exercise<R: Runtime>(app: &AppHandle<R>) {
    let Some((fixture, _)) = options() else {
        return;
    };
    let app = app.clone();
    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wait_until(|| app.state::<AppState>().settings_writer.snapshot().revision == 1);
            record(&app, "native_webview_ipc");
            record(&app, "frontend_renders_native_state");
            let window = app.get_webview_window("main").unwrap();
            on_main(&app, |app| {
                app.get_webview_window("main").unwrap().close().unwrap()
            });
            wait_until(|| !window.is_visible().unwrap());
            record(&app, "close_hides_window");
            if fixture {
                assert_eq!(
                    app.state::<AppState>()
                        .engine
                        .snapshot()
                        .routes
                        .iter()
                        .filter(|r| r.state == PipelineState::Running)
                        .count(),
                    2
                );
                record(&app, "hidden_routes_preserved");
            }
            on_main(&app, |app| crate::tray::dispatch_menu(app, "show"));
            wait_until(|| window.is_visible().unwrap());
            record(&app, "tray_show_restores_window");
            on_main(&app, |app| {
                app.get_webview_window("main")
                    .unwrap()
                    .eval("window.location.reload()")
                    .unwrap()
            });
            wait_until(|| app.state::<AppState>().settings_writer.snapshot().revision == 2);
            record(&app, "reload_reconciles_native_state");
            on_main(&app, |app| crate::tray::dispatch_menu(app, "quit"));
        }));
        if result.is_err() {
            app.state::<SmokeState>()
                .failed
                .store(true, Ordering::Release);
            app.exit(1);
        }
    });
}
pub(crate) fn after_shutdown<R: Runtime>(app: &AppHandle<R>) {
    let Some((fixture, directory)) = options() else {
        return;
    };
    let state = app.state::<AppState>();
    let settings = state.settings_writer.snapshot();
    assert!(state.shutdown_finished.load(Ordering::Acquire));
    let passed =
        settings.saved_revision == 2 && !app.state::<SmokeState>().failed.load(Ordering::Acquire);
    record(app, "quit_flushes_settings_and_finishes_shutdown");
    let report = serde_json::json!({"passed":passed,"fixture":fixture,"evidence":*app.state::<SmokeState>().evidence.lock().unwrap(),"savedRevision":settings.saved_revision,"shutdownFinished":true});
    std::fs::write(
        directory.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
