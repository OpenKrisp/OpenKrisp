use std::{env, path::PathBuf};

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

    embed_windows_test_manifest();
}

fn embed_windows_test_manifest() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    // Tauri's Common Controls v6 feature needs an activation manifest. `tauri-build` embeds it in
    // application binaries, but Cargo's library unit-test executable must receive it separately.
    // A package-wide link argument is required because Cargo does not classify that executable as
    // a named test target.
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("windows-test-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
