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
    let mut attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if targets_windows_msvc() {
        // The explicit linker input below is authoritative for both application and unit-test
        // executables. Keep the generated icon/version resource from defining it a second time.
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }
    tauri_build::try_build(attributes)
        .expect("Tauri context and command permissions must generate");

    embed_windows_manifest();
}

fn targets_windows_msvc() -> bool {
    env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
}

fn embed_windows_manifest() {
    if !targets_windows_msvc() {
        return;
    }

    // Tauri's Common Controls v6 feature needs an activation manifest. `tauri-build` embeds it in
    // application binaries, but Cargo's library unit-test executable must receive it separately.
    // A package-wide link argument is required because Cargo does not classify that executable as
    // a named test target.
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
