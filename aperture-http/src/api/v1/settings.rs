use aperture_auth::{Action, AuthenticatedActor, Object};
use axum::Json;
use axum::extract::{Path, State};
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::dto::{SettingResponse, UpdateSettingRequest};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_settings))
        .routes(routes!(get_setting, update_setting))
}

/// Lists every setting scope with its current value.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_SETTINGS,
    responses((status = 200, description = "Settings", body = [SettingResponse])),
)]
async fn list_settings(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
) -> Result<Json<Vec<SettingResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Setting, Action::Read)
        .await?;
    let entries = state.settings().list().await?;
    let response = entries
        .into_iter()
        .map(|(key, value)| SettingResponse { key, value })
        .collect();
    Ok(Json(response))
}

/// Returns the value of one setting scope.
#[utoipa::path(
    get,
    path = "/{scope}",
    operation_id = operation_ids::GET_SETTING,
    params(("scope" = String, Path, description = "Setting scope")),
    responses(
        (status = 200, description = "Setting value", body = SettingResponse),
        (status = 404, description = "Unknown setting scope"),
    ),
)]
async fn get_setting(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(scope): Path<String>,
) -> Result<Json<SettingResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Setting, Action::Read)
        .await?;
    let value = state.settings().get_value(&scope).await?;
    Ok(Json(SettingResponse { key: scope, value }))
}

/// Replaces the value of one setting scope.
#[utoipa::path(
    put,
    path = "/{scope}",
    operation_id = operation_ids::UPDATE_SETTING,
    params(("scope" = String, Path, description = "Setting scope")),
    request_body = UpdateSettingRequest,
    responses(
        (status = 200, description = "Setting updated", body = SettingResponse),
        (status = 400, description = "Invalid value"),
        (status = 404, description = "Unknown setting scope"),
    ),
)]
async fn update_setting(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(scope): Path<String>,
    Json(request): Json<UpdateSettingRequest>,
) -> Result<Json<SettingResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Setting, Action::Update)
        .await?;
    state
        .settings()
        .set_value(&scope, request.value, auth.actor.id)
        .await?;
    let value: Value = state.settings().get_value(&scope).await?;
    Ok(Json(SettingResponse { key: scope, value }))
}
