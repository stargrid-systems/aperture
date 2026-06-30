use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::{
    ArtifactListParams, ArtifactSummaryResponse, ArtifactVersionResponse, Page, VersionListParams,
    artifact_page, version_page,
};
use crate::error::ApiError;

use super::operation_ids;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_artifacts))
        .routes(routes!(get_artifact))
        .routes(routes!(list_versions))
        .routes(routes!(get_version, delete_version))
}

/// Lists stored artifact keys, each with its newest version.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_ARTIFACTS,
    params(ArtifactListParams),
    responses((status = 200, description = "Artifacts", body = Page<ArtifactSummaryResponse>)),
)]
async fn list_artifacts(
    State(state): State<AppState>,
    Query(params): Query<ArtifactListParams>,
) -> Result<Json<Page<ArtifactSummaryResponse>>, ApiError> {
    let page = state
        .spectra()
        .artifacts()
        .list_artifacts(params.q.as_deref(), &params.to_query())
        .await?;
    Ok(Json(artifact_page(page)))
}

/// Returns one artifact key with its newest version.
#[utoipa::path(
    get,
    path = "/{key}",
    operation_id = operation_ids::GET_ARTIFACT,
    params(("key" = String, Path, description = "Artifact key")),
    responses(
        (status = 200, description = "Artifact", body = ArtifactSummaryResponse),
        (status = 404, description = "Unknown artifact"),
    ),
)]
async fn get_artifact(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<ArtifactSummaryResponse>, ApiError> {
    let artifact = state.spectra().artifacts().artifact(&key).await?;
    artifact
        .map(|key| Json(key.into()))
        .ok_or(ApiError::NOT_FOUND)
}

/// Lists the stored versions of an artifact.
#[utoipa::path(
    get,
    path = "/{key}/versions",
    operation_id = operation_ids::LIST_ARTIFACT_VERSIONS,
    params(
        ("key" = String, Path, description = "Artifact key"),
        VersionListParams,
    ),
    responses((status = 200, description = "Versions", body = Page<ArtifactVersionResponse>)),
)]
async fn list_versions(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(params): Query<VersionListParams>,
) -> Result<Json<Page<ArtifactVersionResponse>>, ApiError> {
    let page = state
        .spectra()
        .artifacts()
        .list_versions(
            &key,
            params.sort(),
            params.media_type.as_deref(),
            params.version.as_deref(),
            &params.to_query(),
        )
        .await?;
    Ok(Json(version_page(page)))
}

/// Returns one stored version.
#[utoipa::path(
    get,
    path = "/{key}/versions/{digest}",
    operation_id = operation_ids::GET_ARTIFACT_VERSION,
    params(
        ("key" = String, Path, description = "Artifact key"),
        ("digest" = String, Path, description = "Content digest"),
    ),
    responses(
        (status = 200, description = "Version", body = ArtifactVersionResponse),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn get_version(
    State(state): State<AppState>,
    Path((key, digest)): Path<(String, String)>,
) -> Result<Json<ArtifactVersionResponse>, ApiError> {
    let version = state.spectra().artifacts().version(&key, &digest).await?;
    version
        .map(|version| Json(version.into()))
        .ok_or(ApiError::NOT_FOUND)
}

/// Evicts one stored version and its blob, if no other version needs it.
#[utoipa::path(
    delete,
    path = "/{key}/versions/{digest}",
    operation_id = operation_ids::DELETE_ARTIFACT_VERSION,
    params(
        ("key" = String, Path, description = "Artifact key"),
        ("digest" = String, Path, description = "Content digest"),
    ),
    responses(
        (status = 204, description = "Version evicted"),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn delete_version(
    State(state): State<AppState>,
    Path((key, digest)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let evicted = state
        .spectra()
        .artifacts()
        .evict_version(&key, &digest)
        .await?;
    if evicted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NOT_FOUND)
    }
}
