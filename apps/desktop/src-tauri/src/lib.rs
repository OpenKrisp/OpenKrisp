#![forbid(unsafe_code)]

mod api_error;
mod commands;
mod config_store;
mod dto;
mod events;
mod state;
mod tray;

use audio_engine::{AudioEngine, AudioEngineConfig, DEFAULT_MODEL_RESOURCE_PATH};
use tauri::{Manager, RunEvent, WindowEvent, path::BaseDirectory};

use crate::{config_store::ConfigStore, events::spawn_event_bridge, state::AppState};

pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("profiles.json");
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
            app.manage(AppState::new(engine, config_store, event_bridge));
            tray::install(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_audio_devices,
            commands::refresh_audio_devices,
            commands::start_route,
            commands::stop_route,
            commands::update_route_settings,
            commands::set_route_bypass,
            commands::get_engine_snapshot,
            commands::list_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::select_profile,
        ])
        .build(tauri::generate_context!())?;

    app.run(|app, event| {
        if matches!(event, RunEvent::ExitRequested { .. })
            && let Some(state) = app.try_state::<AppState>()
        {
            let _ = state.shutdown();
        }
    });
    Ok(())
}
