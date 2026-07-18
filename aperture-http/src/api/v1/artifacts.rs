use std::io;

use aperture_artifacts::{ArtifactError, ArtifactKey};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::TryStreamExt as _;
use tokio::fs::File;
use tokio_util::io::{ReaderStream, StreamReader};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::conditional::{etag_from_digest, format_http_date, matches_etag};
use crate::dto::{
    ArtifactListParams, ArtifactSummaryResponse, ArtifactVersionResponse, Page, VersionListParams,
    artifact_page, version_page,
};
use crate::error::ApiError;

/// Maximum size of a single artifact upload.
///
/// The body is streamed to disk, so this is a backstop against runaway or
/// malicious uploads filling the disk, not a memory cap. We can promote this to
/// a runtime config later.
const MAX_UPLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024; // 2 GiB

pub fn router() -> OpenApiRouter<AppState> {
    use tower_http::limit::RequestBodyLimitLayer;
    OpenApiRouter::new()
        .routes(routes!(list_artifacts))
        .routes(routes!(upload_artifact, get_artifact))
        .routes(routes!(list_versions))
        .routes(routes!(get_version, delete_version))
        .routes(routes!(download_artifact_blob))
        .layer(RequestBodyLimitLayer::new(MAX_UPLOAD_BYTES))
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
    params(("key" = ArtifactKey, Path, description = "Artifact key")),
    responses(
        (status = 200, description = "Artifact", body = ArtifactSummaryResponse),
        (status = 404, description = "Unknown artifact"),
    ),
)]
async fn get_artifact(
    State(state): State<AppState>,
    Path(key): Path<ArtifactKey>,
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
        ("key" = ArtifactKey, Path, description = "Artifact key"),
        VersionListParams,
    ),
    responses((status = 200, description = "Versions", body = Page<ArtifactVersionResponse>)),
)]
async fn list_versions(
    State(state): State<AppState>,
    Path(key): Path<ArtifactKey>,
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
        ("key" = ArtifactKey, Path, description = "Artifact key"),
        ("digest" = String, Path, description = "Content digest"),
    ),
    responses(
        (status = 200, description = "Version", body = ArtifactVersionResponse),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn get_version(
    State(state): State<AppState>,
    Path((key, digest)): Path<(ArtifactKey, String)>,
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
        ("key" = ArtifactKey, Path, description = "Artifact key"),
        ("digest" = String, Path, description = "Content digest"),
    ),
    responses(
        (status = 204, description = "Version evicted"),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn delete_version(
    State(state): State<AppState>,
    Path((key, digest)): Path<(ArtifactKey, String)>,
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

/// Uploads a new artifact version.
///
/// The request body is stored as a content-addressed blob.
#[utoipa::path(
    put,
    path = "/{key}",
    operation_id = operation_ids::UPLOAD_ARTIFACT,
    params(("key" = ArtifactKey, Path, description = "Artifact key")),
    request_body(
        content_type = "application/octet-stream",
        description = "Raw artifact bytes to store",
    ),
    responses(
        (status = 201, description = "Version stored", body = ArtifactVersionResponse),
    ),
)]
async fn upload_artifact(
    State(state): State<AppState>,
    Path(key): Path<ArtifactKey>,
    headers: HeaderMap,
    request: Request,
) -> Result<(StatusCode, Json<ArtifactVersionResponse>), ApiError> {
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    let stream = request
        .into_body()
        .into_data_stream()
        .map_err(io::Error::other);
    let reader = StreamReader::new(stream);
    let artifact = state
        .spectra()
        .artifacts()
        .put(&key, media_type, reader)
        .await?;
    Ok((StatusCode::CREATED, Json(artifact.into())))
}

/// Downloads the blob content of one stored version.
#[utoipa::path(
    get,
    path = "/{key}/versions/{digest}/blob",
    operation_id = operation_ids::DOWNLOAD_ARTIFACT_BLOB,
    params(
        ("key" = ArtifactKey, Path, description = "Artifact key"),
        ("digest" = String, Path, description = "Content digest"),
    ),
    responses(
        (status = 200, description = "Blob content"),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn download_artifact_blob(
    State(state): State<AppState>,
    Path((key, digest)): Path<(ArtifactKey, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let artifacts = state.spectra().artifacts();
    let artifact = artifacts
        .version(&key, &digest)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;

    let etag = etag_from_digest(&artifact.digest);

    if matches_etag(&headers, &etag) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
            ],
        )
            .into_response());
    }

    let located = artifacts
        .locate_version(&key, &digest)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    let file = File::open(&located.path)
        .await
        .map_err(ArtifactError::from)?;
    let content_type = artifact
        .media_type
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let last_modified = format_http_date(artifact.downloaded_at);

    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, artifact.size_bytes)
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::LAST_MODIFIED, last_modified)
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(ApiError::from)
}
