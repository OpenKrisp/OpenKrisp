use audio_engine::EngineCommand;
use audio_protocol::{DeviceInventory, EngineSnapshot, RouteId};
use tauri::State;

use crate::{
    api_error::ApiError,
    config_store::{AudioProfileDto, ProfileCollectionDto},
    dto::{RouteAcceptedDto, SetRouteBypassRequest, StartRouteRequest, UpdateRouteSettingsRequest},
    profile_writer::ProfileMutation,
    state::AppState,
};

#[tauri::command]
pub fn list_audio_devices(state: State<'_, AppState>) -> Result<DeviceInventory, ApiError> {
    Ok(state.engine.snapshot().inventory)
}

#[tauri::command]
pub fn refresh_audio_devices(state: State<'_, AppState>) -> Result<(), ApiError> {
    state
        .engine
        .try_send(EngineCommand::RefreshDevices)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn start_route(
    request: StartRouteRequest,
    state: State<'_, AppState>,
) -> Result<RouteAcceptedDto, ApiError> {
    let config = audio_protocol::RouteConfig::try_from(request)?;
    let route_id = config.id.clone();
    state
        .engine
        .try_send(EngineCommand::StartRoute(config))
        .map_err(ApiError::from)?;
    Ok(RouteAcceptedDto { route_id })
}

#[tauri::command]
pub fn stop_route(route_id: String, state: State<'_, AppState>) -> Result<(), ApiError> {
    let route_id = parse_route_id(route_id)?;
    state
        .engine
        .try_send(EngineCommand::StopRoute(route_id))
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn update_route_settings(
    request: UpdateRouteSettingsRequest,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    let (route_id, settings) = request.validate()?;
    state
        .engine
        .try_send(EngineCommand::UpdateDenoise { route_id, settings })
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn set_route_bypass(
    request: SetRouteBypassRequest,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    let (route_id, bypassed) = request.validate()?;
    state
        .engine
        .try_send(EngineCommand::SetBypass { route_id, bypassed })
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn get_engine_snapshot(state: State<'_, AppState>) -> Result<EngineSnapshot, ApiError> {
    Ok(state.engine.snapshot())
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<ProfileCollectionDto, ApiError> {
    Ok(state.config_store.snapshot())
}

#[tauri::command]
pub async fn save_profile(
    profile: AudioProfileDto,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .profile_writer
        .submit(ProfileMutation::Save(profile))
        .await
}

#[tauri::command]
pub async fn delete_profile(
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .profile_writer
        .submit(ProfileMutation::Delete(profile_id))
        .await
}

#[tauri::command]
pub async fn select_profile(
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .profile_writer
        .submit(ProfileMutation::Select(profile_id))
        .await
}

fn parse_route_id(value: String) -> Result<RouteId, ApiError> {
    RouteId::new(value).map_err(|_| ApiError::invalid_configuration("Route identifier is invalid."))
}
