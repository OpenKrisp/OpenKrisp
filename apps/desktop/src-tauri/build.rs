const COMMANDS: &[&str] = &[
    "list_audio_devices",
    "refresh_audio_devices",
    "start_route",
    "stop_route",
    "update_route_settings",
    "set_route_bypass",
    "get_engine_snapshot",
    "list_profiles",
    "save_profile",
    "delete_profile",
    "select_profile",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("Tauri context and command permissions must generate");
}
