#![forbid(unsafe_code)]

mod api_error;
mod commands;
mod config_store;
#[cfg(test)]
mod control_plane_harness;
mod dto;
mod events;
mod settings_writer;
mod state;
mod tray;

use std::sync::atomic::Ordering;

use audio_engine::{AudioEngine, AudioEngineConfig, DEFAULT_MODEL_RESOURCE_PATH};
use tauri::{Manager, RunEvent, WindowEvent, path::BaseDirectory};

use crate::{config_store::ConfigStore, events::spawn_event_bridge, state::AppState};

fn with_invoke_handler<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        commands::list_audio_devices,
        commands::refresh_audio_devices,
        commands::start_route,
        commands::stop_route,
        commands::update_route_settings,
        commands::set_route_bypass,
        commands::get_engine_snapshot,
        commands::get_settings,
        commands::save_settings,
    ])
}

pub fn run() -> tauri::Result<()> {
    let builder = tauri::Builder::default()
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("settings.json");
            let config_store = ConfigStore::open(config_path);
            let mut engine_config = AudioEngineConfig::default();
            let packaged_model = app
                .path()
                .resolve(DEFAULT_MODEL_RESOURCE_PATH, BaseDirectory::Resource)?;
            if packaged_model.is_file() {
                engine_config.model_path = packaged_model;
            }

            let mut engine = AudioEngine::start(engine_config)?;
            let event_receiver = engine
                .take_event_receiver()
                .ok_or("audio engine event receiver was already taken")?;
            let event_bridge = spawn_event_bridge(app.handle().clone(), event_receiver)?;
            app.manage(AppState::new(engine, config_store, event_bridge)?);
            tray::install(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        });
    let app = with_invoke_handler(builder).build(tauri::generate_context!())?;

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event
            && let Some(state) = app.try_state::<AppState>()
            && !state.shutdown_finished.load(Ordering::Acquire)
        {
            api.prevent_exit();
            if !state.shutdown_started.swap(true, Ordering::AcqRel) {
                let app = app.clone();
                // Keep the event loop alive while native route teardown, event emission and
                // accepted settings writes finish. Re-enter ExitRequested only after joining.
                tauri::async_runtime::spawn_blocking(move || {
                    let result = app.state::<AppState>().shutdown();
                    app.exit(if result.is_ok() { code.unwrap_or(0) } else { 1 });
                });
            }
        }
    });
    Ok(())
}
