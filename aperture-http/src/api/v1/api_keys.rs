use aperture_auth::authz::{self, Permission, Role, Subject};
use aperture_auth::{AuthenticatedActor, RawApiKey};
use aperture_storage::ApiKeyId;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::auth::Require;
use crate::dto::{Page, SimpleListParams};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_api_keys, create_api_key))
        .routes(routes!(delete_api_key))
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiKeyResponse {
    id: ApiKeyId,
    name: String,
    prefix: String,
    last_used_at: Option<Timestamp>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateApiKeyResponse {
    id: ApiKeyId,
    name: String,
    prefix: String,
    /// The full key. Only visible at creation time.
    key: RawApiKey,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    name: String,
    /// Optional role to assign to this key's subject.
    role: Option<Role>,
}

/// Lists API keys for the authenticated actor.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_API_KEYS,
    params(SimpleListParams),
    responses((status = 200, description = "API keys", body = Page<ApiKeyResponse>)),
)]
async fn list_api_keys(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<SimpleListParams>,
) -> Result<Json<Page<ApiKeyResponse>>, ApiError> {
    let page = state
        .storage()
        .api_keys()?
        .list_for_actor(auth.actor.id, &params.to_query())
        .await?;
    Ok(Json(Page::from_storage(page, |k| ApiKeyResponse {
        id: k.id,
        name: k.name,
        prefix: k.prefix,
        last_used_at: k.last_used_at,
    })))
}

/// Creates a new API key for the authenticated actor.
#[utoipa::path(
    post,
    path = "",
    operation_id = operation_ids::CREATE_API_KEY,
    extensions(("x-required-permission" = json!(authz::api_key::Create::PERMISSION))),
    request_body = CreateApiKeyRequest,
    responses((status = 201, description = "API key created", body = CreateApiKeyResponse)),
)]
async fn create_api_key(
    auth: AuthenticatedActor,
    Require(_permit): Require<authz::api_key::Create>,
    State(state): State<AppState>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), ApiError> {
    let (raw_key, api_key) = state
        .auth()
        .create_api_key(auth.actor.id, &request.name)
        .await?;
    if let Some(role) = &request.role
        && let Err(err) = state
            .auth()
            .assign_role(Subject::ApiKey(api_key.id), *role)
            .await
    {
        let repo = state.storage().api_keys()?;
        let _ = repo.delete(api_key.id).await;
        return Err(err.into());
    }
    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            id: api_key.id,
            name: api_key.name,
            prefix: api_key.prefix,
            key: raw_key,
        }),
    ))
}

/// Deletes an API key.
#[utoipa::path(
    delete,
    path = "/{id}",
    operation_id = operation_ids::DELETE_API_KEY,
    extensions(("x-required-permission" = json!(authz::api_key::Delete::PERMISSION))),
    params(("id" = ApiKeyId, Path, description = "API key id")),
    responses(
        (status = 204, description = "API key deleted"),
        (status = 404, description = "Unknown API key"),
    ),
)]
async fn delete_api_key(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(id): Path<ApiKeyId>,
) -> Result<StatusCode, ApiError> {
    let repo = state.storage().api_keys()?;
    let key = repo.get(id).await?.ok_or(ApiError::NOT_FOUND)?;
    // Owners may delete their own keys, so the permission check is
    // conditional and cannot be declared with the Require extractor.
    if key.actor_id != auth.actor.id {
        let _ = state
            .auth()
            .require::<authz::api_key::Delete>(&auth.subject)
            .await?;
    }
    repo.delete(id).await?;
    state.auth().revoke_roles(Subject::ApiKey(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
