use std::{
    num::NonZeroU16,
    path::Path,
    sync::{Arc, Mutex, RwLock, mpsc::sync_channel},
    time::Duration,
};

use audio_engine::{EngineCommand, EngineError};
use audio_protocol::{
    DenoiseSettings, DestinationEndpoint, DeviceId, DeviceInventory, EngineErrorDto, EngineEvent,
    EngineObservation, EngineSnapshot, ErrorCode, PipelineMetrics, PipelineState, ProcessingMode,
    RouteConfig, RouteId, RouteSnapshot, SourceEndpoint,
};

#[test]
fn macos_microphone_metadata_is_wired_into_the_production_bundle() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("bundle config");
    assert_eq!(config["bundle"]["macOS"]["infoPlist"], "Info.plist");
    assert_eq!(
        config["bundle"]["macOS"]["entitlements"],
        "Entitlements.plist"
    );
    let info = include_str!("../Info.plist");
    assert!(info.contains("<key>NSMicrophoneUsageDescription</key>"));
    assert!(info.contains("<string>OpenKrisp uses your microphone"));
    let entitlements = include_str!("../Entitlements.plist");
    assert!(entitlements.contains("<key>com.apple.security.device.audio-input</key>"));
    assert!(entitlements.contains("<true/>"));
}
use serde_json::{Value, json};
use tauri::{
    Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, test::MockRuntime,
    webview::InvokeRequest,
};

use crate::{
    config_store::ConfigStore,
    events::{DEVICES_TOPIC, FAULT_TOPIC, METRICS_TOPIC, STATE_TOPIC, spawn_event_bridge},
    state::{AppState, EngineControl},
    with_invoke_handler,
};

#[derive(Debug)]
struct FakeEngine {
    snapshot: RwLock<EngineSnapshot>,
    commands: Mutex<Vec<EngineCommand>>,
    next_error: Mutex<Option<EngineError>>,
}

impl FakeEngine {
    fn new(snapshot: EngineSnapshot) -> Self {
        Self {
            snapshot: RwLock::new(snapshot),
            commands: Mutex::new(Vec::new()),
            next_error: Mutex::new(None),
        }
    }

    fn fail_next(&self, error: EngineError) {
        *lock_mutex(&self.next_error) = Some(error);
    }

    fn replace_snapshot(&self, snapshot: EngineSnapshot) {
        *write_lock(&self.snapshot) = snapshot;
    }

    fn commands(&self) -> std::sync::MutexGuard<'_, Vec<EngineCommand>> {
        lock_mutex(&self.commands)
    }
}

impl EngineControl for FakeEngine {
    fn try_send(&self, command: EngineCommand) -> Result<(), EngineError> {
        if let Some(error) = lock_mutex(&self.next_error).take() {
            return Err(error);
        }
        lock_mutex(&self.commands).push(command);
        Ok(())
    }

    fn snapshot(&self) -> EngineSnapshot {
        read_lock(&self.snapshot).clone()
    }
}

struct Harness {
    _app: tauri::App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    engine: Arc<FakeEngine>,
}

impl Harness {
    fn new(config_path: &Path) -> Self {
        let engine = Arc::new(FakeEngine::new(snapshot(7, PipelineState::Running)));
        let state = AppState::for_test(engine.clone(), ConfigStore::open(config_path.to_owned()));
        let app = with_invoke_handler(tauri::test::mock_builder())
            .manage(state)
            .manage(crate::updates::UpdateState::new(false))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock Tauri app builds");
        let window = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .expect("mock main webview builds");
        Self {
            _app: app,
            window,
            engine,
        }
    }

    fn invoke(&self, command: &str, body: Value) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: command.to_owned(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: local_app_url().parse().expect("valid local app URL"),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        )
        .map(|body| body.deserialize().expect("successful IPC response is JSON"))
    }
}

#[test]
fn updater_commands_are_disabled_without_a_release_signing_key() {
    let directory = tempfile::tempdir().expect("settings directory");
    let harness = Harness::new(&directory.path().join("settings.json"));
    for command in ["get_update_status", "prepare_update"] {
        let status = harness.invoke(command, json!({})).expect("update status");
        assert_eq!(status["phase"], "disabled");
    }
    assert!(harness.invoke("install_update", json!({})).is_err());
}

#[test]
fn updater_blocks_active_and_queued_audio_and_serializes_quit_with_installation() {
    let directory = tempfile::tempdir().expect("settings directory");
    let harness = Harness::new(&directory.path().join("settings.json"));
    let state = harness._app.state::<AppState>();
    assert!(state.begin_update().is_err());
    harness.engine.replace_snapshot(EngineSnapshot::default());
    harness
        .invoke("start_route", json!({ "request": start_request() }))
        .expect("queued start");
    assert!(state.begin_update().is_err());
    harness
        .invoke("stop_route", json!({ "routeId": "desktop-monitor" }))
        .expect("cancel queued start");
    state.begin_update().expect("idle engine can update");
    assert!(
        harness
            .invoke("start_route", json!({ "request": start_request() }))
            .is_err()
    );
    assert!(
        !state.try_begin_shutdown(),
        "quit cannot terminate an installer"
    );
    state.end_update();
    assert!(
        state.try_begin_shutdown(),
        "cancelled install allows quitting"
    );
    assert!(
        state.begin_update().is_err(),
        "shutdown cannot race installation"
    );
}

#[test]
fn every_desktop_command_crosses_the_real_tauri_invoke_handler() {
    let directory = tempfile::tempdir().expect("temporary settings directory");
    let config_path = directory.path().join("settings.json");
    let harness = Harness::new(&config_path);

    assert_eq!(
        harness
            .invoke("list_audio_devices", json!({}))
            .expect("inventory command succeeds"),
        json!({ "host": "fake", "devices": [] })
    );
    assert_eq!(
        harness
            .invoke("refresh_audio_devices", json!({}))
            .expect("refresh command succeeds"),
        Value::Null
    );

    let first_snapshot = harness
        .invoke("get_engine_snapshot", json!({}))
        .expect("snapshot command succeeds");
    assert_eq!(first_snapshot["revision"], 7);
    assert_eq!(first_snapshot["routes"][0]["state"], "running");
    assert_contains_no_pcm(&first_snapshot);

    let start_response = harness
        .invoke("start_route", json!({ "request": start_request() }))
        .expect("start command succeeds");
    assert_eq!(start_response, json!({ "routeId": "desktop-monitor" }));
    assert_eq!(
        harness
            .invoke(
                "update_route_settings",
                json!({
                    "request": {
                        "routeId": "desktop-monitor",
                        "denoise": denoise_json(18.0)
                    }
                }),
            )
            .expect("update command succeeds"),
        Value::Null
    );
    assert_eq!(
        harness
            .invoke(
                "set_route_bypass",
                json!({ "request": { "routeId": "desktop-monitor", "bypassed": true } }),
            )
            .expect("bypass command succeeds"),
        Value::Null
    );
    assert_eq!(
        harness
            .invoke("stop_route", json!({ "routeId": "desktop-monitor" }))
            .expect("stop command succeeds"),
        Value::Null
    );

    let commands = harness.engine.commands();
    assert_eq!(commands.len(), 5);
    assert!(matches!(commands[0], EngineCommand::RefreshDevices));
    assert!(matches!(
        &commands[1],
        EngineCommand::StartRoute(config)
            if config.id.as_str() == "desktop-monitor"
                && config.processing_mode == ProcessingMode::SpeechDenoise
                && config.echo_cancellation
    ));
    assert!(matches!(
        &commands[2],
        EngineCommand::UpdateDenoise { route_id, settings }
            if route_id.as_str() == "desktop-monitor"
                && settings.attenuation_limit_db == Some(18.0)
    ));
    assert!(matches!(
        &commands[3],
        EngineCommand::SetBypass { route_id, bypassed: true }
            if route_id.as_str() == "desktop-monitor"
    ));
    assert!(matches!(
        &commands[4],
        EngineCommand::StopRoute(route_id) if route_id.as_str() == "desktop-monitor"
    ));

    drop(commands);

    let mut settings = harness
        .invoke("get_settings", json!({}))
        .expect("settings load")["settings"]
        .clone();
    assert_eq!(
        settings["microphone"]["denoise"],
        json!({ "enabled": true, "attenuationLimitDb": null, "postFilterEnabled": false })
    );
    assert_eq!(
        settings["incoming"]["denoise"],
        settings["microphone"]["denoise"]
    );
    settings["microphone"]["inputId"] = json!("fake:input");
    settings["microphone"]["echoCancellation"] = json!(false);
    settings["incoming"]["outputId"] = json!("fake:output");
    settings["incoming"]["targetLatencyMs"] = json!(65);
    let accepted = harness
        .invoke(
            "save_settings",
            json!({ "request": {
                "settings": settings, "revision": 1, "replaceUnreadable": false
            }}),
        )
        .expect("settings accepted");
    assert_eq!(accepted["revision"], 1);
    assert_contains_no_pcm(&accepted);
    assert_eq!(
        harness.engine.commands().len(),
        5,
        "persistence never controls audio"
    );
    harness
        ._app
        .state::<AppState>()
        .shutdown()
        .expect("flush before exit");
    drop(harness);
    let reopened = Harness::new(&config_path);
    let restored = reopened
        .invoke("get_settings", json!({}))
        .expect("saved settings reopen");
    assert_eq!(restored["settings"], settings);
    assert_eq!(restored["revision"], 0, "revisions are process-local");
    assert!(
        reopened.engine.commands().is_empty(),
        "restoration never starts audio"
    );
}

#[test]
fn virtual_microphone_request_crosses_ipc_without_becoming_a_physical_output() {
    let directory = tempfile::tempdir().expect("temporary settings");
    let harness = Harness::new(&directory.path().join("settings.json"));
    let mut request = start_request();
    request["destinationKind"] = json!("virtual_microphone");
    request["destinationDeviceId"] = json!("macos:openkrisp-microphone");
    harness
        .invoke("start_route", json!({ "request": request }))
        .expect("accepted");
    let commands = harness.engine.commands();
    let EngineCommand::StartRoute(config) = &commands[0] else {
        panic!("start command")
    };
    assert!(
        matches!(&config.destination, DestinationEndpoint::VirtualMicrophone(id) if id.as_str() == "macos:openkrisp-microphone")
    );
    drop(commands);
    request["destinationKind"] = json!("system_loopback");
    assert!(
        harness
            .invoke("start_route", json!({ "request": request }))
            .is_err()
    );
    assert_eq!(harness.engine.commands().len(), 1);
}

#[test]
fn incoming_ipc_defaults_to_bypass_and_rejects_pcm_and_virtual_destinations() {
    let directory = tempfile::tempdir().expect("temporary settings");
    let harness = Harness::new(&directory.path().join("settings.json"));
    let mut request = start_request();
    request["sourceKind"] = json!("system_loopback");
    request["sourceDeviceId"] = json!("macos:openkrisp-speaker");
    request.as_object_mut().expect("object").remove("mode");
    harness
        .invoke("start_route", json!({"request": request}))
        .expect("accepted");
    {
        let commands = harness.engine.commands();
        let EngineCommand::StartRoute(config) = &commands[0] else {
            panic!("start command");
        };
        assert!(matches!(config.source, SourceEndpoint::SystemLoopback(_)));
        assert_eq!(config.processing_mode, ProcessingMode::AlignedBypass);
        assert!(!config.echo_cancellation);
    }
    request["pcm"] = json!([0.1, 0.2]);
    assert!(
        harness
            .invoke("start_route", json!({"request": request}))
            .is_err()
    );
    request.as_object_mut().expect("object").remove("pcm");
    request["sourceKind"] = json!("application_stream");
    assert!(
        harness
            .invoke("start_route", json!({"request": request}))
            .is_err()
    );
    request["sourceKind"] = json!("system_loopback");
    request["destinationKind"] = json!("virtual_microphone");
    assert!(
        harness
            .invoke("start_route", json!({"request": request}))
            .is_err()
    );
    assert_eq!(harness.engine.commands().len(), 1);
}

#[test]
fn aec_opt_out_and_metadata_cross_the_real_command_boundary_without_pcm() {
    let directory = tempfile::tempdir().unwrap();
    let harness = Harness::new(&directory.path().join("settings.json"));
    let mut request = start_request();
    request["echoCancellation"] = json!(false);
    harness
        .invoke("start_route", json!({"request": request}))
        .unwrap();
    assert!(
        matches!(&harness.engine.commands()[0], EngineCommand::StartRoute(config) if !config.echo_cancellation)
    );
    let mut observed = snapshot(8, PipelineState::Running);
    observed.routes[0].config.echo_cancellation = true;
    observed.routes[0].echo_cancellation = Some(audio_protocol::EchoCancellationSnapshot {
        status: audio_protocol::EchoCancellationStatus::WaitingReference,
        processed_frames: 100,
        fallback_frames: 20,
        resets: 1,
        native_errors: 0,
        max_processing_us: 500,
        missed_deadlines: 0,
    });
    harness.engine.replace_snapshot(observed);
    let payload = harness.invoke("get_engine_snapshot", json!({})).unwrap();
    assert_eq!(
        payload["routes"][0]["echoCancellation"]["status"],
        "waiting_reference"
    );
    assert_eq!(
        payload["routes"][0]["echoCancellation"]["fallbackFrames"],
        20
    );
    assert_contains_no_pcm(&payload);
    request["referencePcm"] = json!([0.1]);
    assert!(
        harness
            .invoke("start_route", json!({"request": request}))
            .is_err()
    );
}

#[test]
fn invoke_errors_are_typed_and_snapshots_reconcile_without_event_history() {
    let directory = tempfile::tempdir().expect("temporary settings directory");
    let harness = Harness::new(&directory.path().join("settings.json"));

    harness.engine.fail_next(EngineError::CommandQueueFull);
    let busy = harness
        .invoke("refresh_audio_devices", json!({}))
        .expect_err("queue saturation returns an IPC error");
    assert_eq!(busy["code"], "command_queue_full");
    assert!(
        busy["message"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let mut invalid_request = start_request();
    invalid_request["targetLatencyMs"] = json!(0);
    let invalid = harness
        .invoke("start_route", json!({ "request": invalid_request }))
        .expect_err("semantic validation returns an IPC error");
    assert_eq!(invalid["code"], "invalid_configuration");

    assert!(
        harness
            .invoke("command_not_in_the_handler", json!({}))
            .is_err(),
        "an unregistered generic command must remain unavailable"
    );

    harness
        .engine
        .replace_snapshot(snapshot(11, PipelineState::Stopped));
    let reconciled = harness
        .invoke("get_engine_snapshot", json!({}))
        .expect("snapshot reconciliation succeeds without prior events");
    assert_eq!(reconciled["revision"], 11);
    assert_eq!(reconciled["routes"][0]["state"], "stopped");
    assert_contains_no_pcm(&reconciled);
}

#[test]
fn bounded_event_bridge_delivers_every_low_rate_topic_and_then_joins() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock event app builds");
    let (observed_tx, observed_rx) = sync_channel(4);
    for topic in [STATE_TOPIC, DEVICES_TOPIC, METRICS_TOPIC, FAULT_TOPIC] {
        let observed_tx = observed_tx.clone();
        app.listen(topic, move |event| {
            let payload: Value = serde_json::from_str(event.payload()).expect("event payload JSON");
            observed_tx
                .send((topic, payload))
                .expect("test observer remains connected");
        });
    }
    drop(observed_tx);

    let (event_tx, event_rx) = sync_channel(4);
    let bridge = spawn_event_bridge(app.handle().clone(), event_rx).expect("event bridge starts");
    let route_id = RouteId::new("desktop-monitor").expect("valid route ID");
    let expected = [
        (
            STATE_TOPIC,
            EngineEvent::StateChanged {
                route_id: route_id.clone(),
                state: PipelineState::Running,
            },
        ),
        (DEVICES_TOPIC, EngineEvent::DevicesChanged),
        (
            METRICS_TOPIC,
            EngineEvent::Metrics {
                route_id: route_id.clone(),
                metrics: PipelineMetrics {
                    input_peak: 0.25,
                    ..PipelineMetrics::default()
                },
            },
        ),
        (
            FAULT_TOPIC,
            EngineEvent::Fault {
                route_id: Some(route_id),
                error: EngineErrorDto::new(ErrorCode::DeviceDisconnected, "device disconnected"),
                recoverable: true,
            },
        ),
    ];

    for (_, event) in &expected {
        event_tx
            .send(EngineObservation {
                revision: 7,
                event: event.clone(),
            })
            .expect("event queue accepts event");
    }
    for (expected_topic, expected_event) in expected {
        let (topic, payload) = observed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("event reaches the Tauri event manager");
        assert_eq!(topic, expected_topic);
        assert_eq!(payload["revision"], 7);
        assert_eq!(
            serde_json::from_value::<EngineEvent>(payload.clone()).expect("event DTO decodes"),
            expected_event
        );
        assert_contains_no_pcm(&payload);
    }

    drop(event_tx);
    bridge.join().expect("event bridge joins after disconnect");
}

#[test]
fn production_capability_exposes_only_named_commands_and_event_observation() {
    let capability: Value = serde_json::from_str(include_str!("../capabilities/main.json"))
        .expect("capability is valid JSON");
    let permissions = capability["permissions"]
        .as_array()
        .expect("capability permissions are an array")
        .iter()
        .map(|permission| permission.as_str().expect("permission is a string"))
        .collect::<Vec<_>>();

    assert_eq!(permissions.len(), 15);
    assert!(permissions.contains(&"core:event:allow-listen"));
    assert!(permissions.contains(&"core:event:allow-unlisten"));
    assert!(!permissions.iter().any(|permission| {
        permission.contains("event:default")
            || permission.contains("allow-emit")
            || permission.contains("shell")
            || permission.contains("fs:")
            || permission.starts_with("updater:")
    }));
    for command in [
        "list-audio-devices",
        "refresh-audio-devices",
        "start-route",
        "stop-route",
        "update-route-settings",
        "set-route-bypass",
        "get-engine-snapshot",
        "get-settings",
        "save-settings",
        "get-update-status",
        "prepare-update",
        "install-update",
        "set-app-language",
    ] {
        assert!(permissions.contains(&format!("allow-{command}").as_str()));
    }
}

fn snapshot(revision: u64, state: PipelineState) -> EngineSnapshot {
    EngineSnapshot {
        routing: Default::default(),
        revision,
        inventory: DeviceInventory {
            host: "fake".to_owned(),
            devices: Vec::new(),
        },
        routes: vec![RouteSnapshot::pending(route_config(), state)],
        ..EngineSnapshot::default()
    }
}

fn route_config() -> RouteConfig {
    RouteConfig {
        id: RouteId::new("desktop-monitor").expect("valid route ID"),
        source: SourceEndpoint::PhysicalInput(DeviceId::new("fake:input").expect("valid input ID")),
        destination: DestinationEndpoint::PhysicalOutput(
            DeviceId::new("fake:output").expect("valid output ID"),
        ),
        echo_cancellation: false,
        processing_mode: ProcessingMode::SpeechDenoise,
        target_latency_ms: NonZeroU16::new(40).expect("non-zero latency"),
        denoise: DenoiseSettings::default(),
    }
}

fn start_request() -> Value {
    json!({
        "routeId": "desktop-monitor",
        "sourceDeviceId": "fake:input",
        "destinationDeviceId": "fake:output",
        "mode": "speech_denoise",
        "targetLatencyMs": 40,
        "denoise": denoise_json(24.0)
    })
}

fn denoise_json(attenuation_limit_db: f32) -> Value {
    json!({
        "enabled": true,
        "attenuationLimitDb": attenuation_limit_db,
        "postFilterEnabled": true
    })
}

fn assert_contains_no_pcm(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    !matches!(
                        key.to_ascii_lowercase().as_str(),
                        "pcm" | "samples" | "audiodata" | "sampledata" | "samplebuffer"
                    ),
                    "IPC payload contains forbidden PCM field {key}"
                );
                assert_contains_no_pcm(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_contains_no_pcm),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn local_app_url() -> &'static str {
    "tauri://localhost"
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// A single literal is consumed by Rust serialization/IPC tests and TypeScript consumers.
// Regenerate deliberately with OPENKRISP_UPDATE_CONTRACT=1; CI never updates it.
#[test]
fn shared_desktop_contract_matches_rust_and_real_ipc() {
    let directory = tempfile::tempdir().unwrap();
    let harness = Harness::new(&directory.path().join("settings.json"));
    harness.engine.replace_snapshot(contract_snapshot());
    let route_id = RouteId::new("desktop-monitor").unwrap();
    let events = [
        (STATE_TOPIC, EngineEvent::StateChanged { route_id: route_id.clone(), state: PipelineState::Recovering }),
        (DEVICES_TOPIC, EngineEvent::DevicesChanged),
        (METRICS_TOPIC, EngineEvent::Metrics { route_id: route_id.clone(), metrics: PipelineMetrics::default() }),
        (FAULT_TOPIC, EngineEvent::Fault { route_id: Some(route_id), error: EngineErrorDto { code: ErrorCode::DeviceDisconnected, message: "Fixture device disappeared.".into(), details: None, suggested_action: Some("Reconnect the device.".into()) }, recoverable: true }),
    ].into_iter().enumerate().map(|(index, (topic, event))| json!({"topic": topic, "payload": EngineObservation { revision: index as u64 + 8, event }})).collect::<Vec<_>>();
    let calls = vec![
        json!(["list_audio_devices"]),
        json!(["refresh_audio_devices"]),
        json!(["start_route", {"request": start_request()}]),
        json!(["update_route_settings", {"request": {"routeId":"desktop-monitor","denoise":denoise_json(18.0)}}]),
        json!(["set_route_bypass", {"request": {"routeId":"desktop-monitor","bypassed":true}}]),
        json!(["stop_route", {"routeId":"desktop-monitor"}]),
        json!(["get_engine_snapshot"]),
        json!(["get_settings"]),
    ];
    for call in &calls {
        harness
            .invoke(
                call[0].as_str().unwrap(),
                call.get(1).cloned().unwrap_or(json!({})),
            )
            .unwrap();
    }
    let mut invalid = start_request();
    invalid["targetLatencyMs"] = json!(0);
    let value = json!({
        "snapshot": harness.invoke("get_engine_snapshot", json!({})).unwrap(),
        "settings": harness.invoke("get_settings", json!({})).unwrap(),
        "error": harness.invoke("start_route", json!({"request":invalid})).unwrap_err(),
        "events":events,
        "calls":calls,
        "updates": crate::updates::contract_statuses(),
        "pipelineStates": [PipelineState::Stopped, PipelineState::Starting, PipelineState::Running, PipelineState::Reconfiguring, PipelineState::Recovering, PipelineState::Failed, PipelineState::Stopping],
    });
    assert_contains_no_pcm(&value);
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures/desktop-contract.ts");
    if std::env::var_os("OPENKRISP_UPDATE_CONTRACT").is_some() {
        std::fs::write(
            &path,
            format!(
                "export default {} as const;\n",
                serde_json::to_string_pretty(&value).unwrap()
            ),
        )
        .unwrap();
    }
    let source = std::fs::read_to_string(path).unwrap();
    let expected: Value = serde_json::from_str(
        source
            .strip_prefix("export default ")
            .unwrap()
            .strip_suffix(" as const;\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        value, expected,
        "Review contract drift in both language consumers before regenerating"
    );
}

fn contract_snapshot() -> EngineSnapshot {
    use audio_protocol::*;
    let mut observed = snapshot(7, PipelineState::Running);
    let format = AudioFormat {
        sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
        channels: ChannelCount::new(1).unwrap(),
        sample_format: SampleFormat::F32,
        buffer_size_frames: Some(BufferSizeFrames::new(480).unwrap()),
    };
    let device = AudioDeviceDescriptor {
        id: DeviceId::new("fake:input").unwrap(),
        kind: DeviceKind::Physical,
        name: "Fixture microphone".into(),
        host: "fake".into(),
        manufacturer: Some("Fixture manufacturer".into()),
        driver: Some("fixture.driver".into()),
        supports_input: true,
        supports_output: false,
        is_default_input: true,
        is_default_output: false,
        default_input_format: Some(format.clone()),
        default_output_format: None,
    };
    observed.inventory.devices.push(device.clone());
    let microphone = &mut observed.routes[0];
    microphone.input_device = Some(device);
    microphone.input_format = Some(format.clone());
    microphone.output_format = Some(format);
    microphone.model = Some(ModelInfo {
        id: "DeepFilterNet3_ll".into(),
        sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
        channels: 1,
        frame_samples_per_channel: 480,
        algorithmic_latency_samples: 480,
    });
    microphone.metrics.input_peak = 0.25;
    microphone.metrics.inference_p99_us = 1250;
    microphone.metrics.drift_correction_ppm = -100;
    microphone.metrics.estimated_latency_ms = 42.5;
    microphone.echo_cancellation = Some(EchoCancellationSnapshot {
        status: EchoCancellationStatus::Running,
        processed_frames: 42,
        fallback_frames: 3,
        resets: 1,
        native_errors: 0,
        max_processing_us: 200,
        missed_deadlines: 0,
    });
    microphone.input_stream.callbacks = 45;
    microphone.input_stream.last_issue = Some("fixture xrun".into());
    microphone.successful_recoveries = 1;
    let mut incoming_config = route_config();
    incoming_config.id = RouteId::new("desktop-incoming").unwrap();
    incoming_config.source =
        SourceEndpoint::SystemLoopback(LoopbackEndpointId::new("fake:loopback").unwrap());
    incoming_config.processing_mode = ProcessingMode::AlignedBypass;
    let mut incoming = RouteSnapshot::pending(incoming_config, PipelineState::Failed);
    incoming.fault = Some(EngineErrorDto {
        code: ErrorCode::DeviceDisconnected,
        message: "Fixture headphones disappeared.".into(),
        details: Some("fixture stream 2".into()),
        suggested_action: Some("Reconnect headphones.".into()),
    });
    observed.last_fault = incoming.fault.clone();
    observed.routes.push(incoming);
    observed.routing.capabilities.physical_input = true;
    observed.routing.capabilities.physical_output = true;
    observed.routing.capabilities.system_loopback = true;
    observed.routing.capabilities.virtual_microphone = true;
    observed.routing.capabilities.supports_render_reference = true;
    observed.routing.endpoints.push(PlatformEndpoint {
        id: VirtualEndpointId::new("fake:virtual-microphone").unwrap(),
        name: "Fixture virtual microphone".into(),
        availability: EndpointAvailability::Ready,
        can_create: false,
        playback_device_id: Some(DeviceId::new("fake:virtual-output").unwrap()),
        capture_device_id: Some(DeviceId::new("fake:virtual-input").unwrap()),
        ancestry: EndpointAncestry {
            recursive_paths: vec![],
            complete: true,
        },
        instructions: vec!["Fixture setup instructions.".into()],
        setup_url: "https://example.com/fixture".into(),
    });
    observed.routing.loopback_endpoints = observed.routing.endpoints.clone();
    observed
}
