use aperture_auth::{Action, AuthenticatedActor, Object};
use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::dto::{Page, SettingListParams, SettingResponse, UpdateSettingRequest};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_settings))
        .routes(routes!(get_setting, update_setting))
}

/// Lists setting keys with their current values, paginated.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_SETTINGS,
    params(SettingListParams),
    responses((status = 200, description = "Settings", body = Page<SettingResponse>)),
)]
async fn list_settings(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<SettingListParams>,
) -> Result<Json<Page<SettingResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Setting, Action::Read)
        .await?;
    let page = state.settings().list(&params.to_query()).await?;
    Ok(Json(Page::from_storage(page, |(key, value)| {
        SettingResponse { key, value }
    })))
}

/// Returns the value of one setting key.
#[utoipa::path(
    get,
    path = "/{key}",
    operation_id = operation_ids::GET_SETTING,
    params(("key" = String, Path, description = "Setting key")),
    responses(
        (status = 200, description = "Setting value", body = SettingResponse),
        (status = 404, description = "Unknown setting key"),
    ),
)]
async fn get_setting(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<SettingResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Setting, Action::Read)
        .await?;
    let value = state.settings().get_value(&key).await?;
    Ok(Json(SettingResponse { key, value }))
}

/// Replaces the value of one setting key.
#[utoipa::path(
    put,
    path = "/{key}",
    operation_id = operation_ids::UPDATE_SETTING,
    params(("key" = String, Path, description = "Setting key")),
    request_body = UpdateSettingRequest,
    responses(
        (status = 200, description = "Setting updated", body = SettingResponse),
        (status = 400, description = "Invalid value"),
        (status = 404, description = "Unknown setting key"),
    ),
)]
async fn update_setting(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(request): Json<UpdateSettingRequest>,
) -> Result<Json<SettingResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Setting, Action::Update)
        .await?;
    state
        .settings()
        .set_value(&key, request.value, auth.actor.id)
        .await?;
    let value: Value = state.settings().get_value(&key).await?;
    Ok(Json(SettingResponse { key, value }))
}
