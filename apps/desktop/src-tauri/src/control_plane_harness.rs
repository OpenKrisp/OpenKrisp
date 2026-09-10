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
    Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder, test::MockRuntime,
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
fn every_desktop_command_crosses_the_real_tauri_invoke_handler() {
    let directory = tempfile::tempdir().expect("temporary profile directory");
    let config_path = directory.path().join("profiles.json");
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

    let profile = json!({
        "id": "office",
        "name": "Office",
        "sourceDevicePreference": "fake:input",
        "destinationDevicePreference": "fake:output",
        "targetLatencyMs": 60,
        "denoise": denoise_json(24.0)
    });
    assert_eq!(
        harness
            .invoke("save_profile", json!({ "profile": profile }))
            .expect("profile save succeeds"),
        Value::Null
    );
    assert_eq!(
        harness
            .invoke("select_profile", json!({ "profileId": "office" }))
            .expect("profile selection succeeds"),
        Value::Null
    );
    let profiles = harness
        .invoke("list_profiles", json!({}))
        .expect("profile listing succeeds");
    assert_eq!(profiles["schemaVersion"], 2);
    assert_eq!(profiles["selectedProfileId"], "office");
    assert_eq!(profiles["profiles"].as_array().map(Vec::len), Some(2));

    drop(harness);
    let reopened = Harness::new(&config_path);
    let profiles = reopened
        .invoke("list_profiles", json!({}))
        .expect("persisted profiles reopen through IPC");
    assert_eq!(profiles["selectedProfileId"], "office");
    assert_eq!(
        reopened
            .invoke("delete_profile", json!({ "profileId": "office" }))
            .expect("profile deletion succeeds"),
        Value::Null
    );
    let profiles = reopened
        .invoke("list_profiles", json!({}))
        .expect("profiles reconcile after deletion");
    assert_eq!(profiles["selectedProfileId"], "default");
    assert_eq!(profiles["profiles"].as_array().map(Vec::len), Some(1));
}

#[test]
fn virtual_microphone_request_crosses_ipc_without_becoming_a_physical_output() {
    let directory = tempfile::tempdir().expect("temporary profiles");
    let harness = Harness::new(&directory.path().join("profiles.json"));
    let mut request = start_request();
    request["destinationKind"] = json!("virtual_microphone");
    request["destinationDeviceId"] = json!("macos:blackhole-2ch");
    harness
        .invoke("start_route", json!({ "request": request }))
        .expect("accepted");
    let commands = harness.engine.commands();
    let EngineCommand::StartRoute(config) = &commands[0] else {
        panic!("start command")
    };
    assert!(
        matches!(&config.destination, DestinationEndpoint::VirtualMicrophone(id) if id.as_str() == "macos:blackhole-2ch")
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
    let directory = tempfile::tempdir().expect("temporary profiles");
    let harness = Harness::new(&directory.path().join("profiles.json"));
    let mut request = start_request();
    request["sourceKind"] = json!("system_loopback");
    request["sourceDeviceId"] = json!("macos:blackhole-2ch");
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
fn invoke_errors_are_typed_and_snapshots_reconcile_without_event_history() {
    let directory = tempfile::tempdir().expect("temporary profile directory");
    let harness = Harness::new(&directory.path().join("profiles.json"));

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

    assert_eq!(permissions.len(), 13);
    assert!(permissions.contains(&"core:event:allow-listen"));
    assert!(permissions.contains(&"core:event:allow-unlisten"));
    assert!(!permissions.iter().any(|permission| {
        permission.contains("event:default")
            || permission.contains("allow-emit")
            || permission.contains("shell")
            || permission.contains("fs:")
    }));
    for command in [
        "list-audio-devices",
        "refresh-audio-devices",
        "start-route",
        "stop-route",
        "update-route-settings",
        "set-route-bypass",
        "get-engine-snapshot",
        "list-profiles",
        "save-profile",
        "delete-profile",
        "select-profile",
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
