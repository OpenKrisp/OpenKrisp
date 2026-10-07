const COMMANDS: &[&str] = &[
    "list_audio_devices",
    "refresh_audio_devices",
    "start_route",
    "stop_route",
    "update_route_settings",
    "set_route_bypass",
    "get_engine_snapshot",
    "get_settings",
    "save_settings",
    "get_update_status",
    "prepare_update",
    "install_update",
];

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("Cargo target OS");
    assert!(
        matches!(target_os.as_str(), "macos" | "linux"),
        "OpenKrisp desktop supports only macOS and Linux."
    );
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes)
        .expect("Tauri context and command permissions must generate");
}
