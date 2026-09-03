use aperture_auth::AuthenticatedActor;
use aperture_auth::authz::{self, Permission};
use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::auth::Require;
use crate::dto::{
    Page, SettingDefinitionResponse, SettingDefinitionSummary, SettingResponse, SimpleListParams,
    UpdateSettingRequest,
};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_settings))
        .routes(routes!(get_setting, update_setting))
}

pub fn definitions_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_setting_definitions))
        .routes(routes!(get_setting_definition))
}

/// Lists every setting key with its current value.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_SETTINGS,
    extensions(("x-required-permission" = json!(authz::setting::Read::PERMISSION))),
    responses((status = 200, description = "Settings", body = [SettingResponse])),
)]
async fn list_settings(
    Require(_permit): Require<authz::setting::Read>,
    State(state): State<AppState>,
) -> Result<Json<Vec<SettingResponse>>, ApiError> {
    let entries = state.settings().list().await?;
    let response = entries
        .into_iter()
        .map(|(key, value)| SettingResponse { key, value })
        .collect();
    Ok(Json(response))
}

/// Returns the value of one setting key.
#[utoipa::path(
    get,
    path = "/{key}",
    operation_id = operation_ids::GET_SETTING,
    extensions(("x-required-permission" = json!(authz::setting::Read::PERMISSION))),
    params(("key" = String, Path, description = "Setting key")),
    responses(
        (status = 200, description = "Setting value", body = SettingResponse),
        (status = 404, description = "Unknown setting key"),
    ),
)]
async fn get_setting(
    Require(_permit): Require<authz::setting::Read>,
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<SettingResponse>, ApiError> {
    let value = state.settings().get_value(&key).await?;
    Ok(Json(SettingResponse { key, value }))
}

/// Replaces the value of one setting key.
#[utoipa::path(
    put,
    path = "/{key}",
    operation_id = operation_ids::UPDATE_SETTING,
    extensions(("x-required-permission" = json!(authz::setting::Update::PERMISSION))),
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
    Require(_permit): Require<authz::setting::Update>,
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(request): Json<UpdateSettingRequest>,
) -> Result<Json<SettingResponse>, ApiError> {
    state
        .settings()
        .set_value(&key, request.value, auth.actor.id)
        .await?;
    let value: Value = state.settings().get_value(&key).await?;
    Ok(Json(SettingResponse { key, value }))
}

/// Lists the registered setting definitions.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_SETTING_DEFINITIONS,
    params(SimpleListParams),
    security(()),
    responses((
        status = 200,
        description = "Setting definitions",
        body = Page<SettingDefinitionSummary>,
    )),
)]
async fn list_setting_definitions(
    State(state): State<AppState>,
    Query(params): Query<SimpleListParams>,
) -> Result<Json<Page<SettingDefinitionSummary>>, ApiError> {
    let page = state
        .settings()
        .registry()
        .list(&params.to_query())
        .map_err(|_| ApiError::BAD_REQUEST)?;
    Ok(Json(Page::from_registry(page, |definition| {
        SettingDefinitionSummary {
            key: definition.key().to_owned(),
        }
    })))
}

/// Returns one registered setting definition with its full JSON Schema.
#[utoipa::path(
    get,
    path = "/{key}",
    operation_id = operation_ids::GET_SETTING_DEFINITION,
    params(("key" = String, Path, description = "Setting definition key")),
    security(()),
    responses(
        (status = 200, description = "Setting definition", body = SettingDefinitionResponse),
        (status = 404, description = "Unknown setting definition key"),
    ),
)]
async fn get_setting_definition(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<SettingDefinitionResponse>, ApiError> {
    let definition = state
        .settings()
        .registry()
        .get(&key)
        .ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(SettingDefinitionResponse {
        key: definition.key().to_owned(),
        value_schema: definition.value_schema(),
    }))
}
