#![forbid(unsafe_code)]

mod api_error;
mod commands;
mod config_store;
#[cfg(test)]
mod control_plane_harness;
mod dto;
mod events;
mod settings_writer;
#[cfg(feature = "desktop-smoke")]
mod smoke;
mod state;
mod tray;
mod updates;

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
        updates::get_update_status,
        updates::prepare_update,
        updates::install_update,
    ])
}

pub fn run() -> tauri::Result<()> {
    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(updates::PUBLIC_KEY)
                .build(),
        )
        .setup(|app| {
            app.manage(updates::UpdateState::new(
                !cfg!(debug_assertions) && !updates::PUBLIC_KEY.is_empty(),
            ));
            let config_path = app.path().app_config_dir()?.join("settings.json");
            #[cfg(feature = "desktop-smoke")]
            let config_path = smoke::settings_path().unwrap_or(config_path);
            let config_store = ConfigStore::open(config_path.clone());
            let mut engine_config = AudioEngineConfig::default();
            let packaged_model = app
                .path()
                .resolve(DEFAULT_MODEL_RESOURCE_PATH, BaseDirectory::Resource)?;
            if packaged_model.is_file() {
                engine_config.model_path = packaged_model.clone();
            }

            #[cfg(feature = "desktop-smoke")]
            {
                smoke::validate_resource(app, &packaged_model)?;
                if smoke::install_fixture_state(app, &config_path) {
                    tray::install(app)?;
                    return Ok(());
                }
            }
            let mut engine = AudioEngine::start(engine_config)?;
            let event_receiver = engine
                .take_event_receiver()
                .ok_or("audio engine event receiver was already taken")?;
            let event_bridge = spawn_event_bridge(app.handle().clone(), event_receiver)?;
            app.manage(AppState::new(engine, config_store, event_bridge)?);
            tray::install(app)?;
            updates::start_scheduler(app.handle());
            Ok(())
        })
        .on_page_load(|webview, payload| {
            #[cfg(feature = "desktop-smoke")]
            smoke::page_loaded(webview, payload);
            #[cfg(not(feature = "desktop-smoke"))]
            let _ = (webview, payload);
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        });
    let app = with_invoke_handler(builder).build(tauri::generate_context!())?;

    app.run(|app, event| {
        #[cfg(feature = "desktop-smoke")]
        if matches!(event, RunEvent::Ready) {
            smoke::exercise(app);
        }

        if let RunEvent::ExitRequested { api, code, .. } = event
            && let Some(state) = app.try_state::<AppState>()
            && !state.shutdown_finished.load(Ordering::Acquire)
        {
            api.prevent_exit();
            if state.try_begin_shutdown() {
                if let Some(updates) = app.try_state::<updates::UpdateState>() {
                    updates.stop_scheduler();
                }
                let app = app.clone();
                // Keep the event loop alive while native route teardown, event emission and
                // accepted settings writes finish. Re-enter ExitRequested only after joining.
                tauri::async_runtime::spawn_blocking(move || {
                    let result = app.state::<AppState>().shutdown();
                    #[cfg(feature = "desktop-smoke")]
                    smoke::after_shutdown(&app);
                    app.exit(if result.is_ok() { code.unwrap_or(0) } else { 1 });
                });
            }
        }
    });
    Ok(())
}
