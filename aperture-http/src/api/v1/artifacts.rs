//! Artifact catalog HTTP endpoints.

use std::io;

use aperture_artifacts::ArtifactError;
use aperture_auth::{Action, AuthenticatedActor, Object};
use aperture_storage::{ArtifactKey, Digest, MediaType};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::TryStreamExt as _;
use tokio::fs::File;
use tokio_util::io::{ReaderStream, StreamReader};
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::conditional::{Etag, HttpDate};
use crate::dto::{
    ArtifactListParams, ArtifactSummaryResponse, ArtifactVersionResponse, Page, VersionListParams,
};
use crate::error::ApiError;

/// Maximum request body size for artifact ingestion.
///
/// The body is streamed to disk, so this guards against runaway or malicious
/// uploads filling the disk, not against memory exhaustion. The constant is
/// larger than `i32::MAX`, so it assumes a 64-bit target. We can promote this
/// to a runtime config later.
///
/// Because uploads are bounded by this limit, every stored `size_bytes` fits
/// comfortably in `u64` and therefore in an HTTP `Content-Length`. The
/// download handler relies on that invariant when it stamps
/// `Content-Length: artifact.size_bytes` on the response.
const MAX_UPLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024; // 2 GiB

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_artifacts))
        .routes(routes!(get_artifact))
        .routes(routes!(list_versions))
        .routes(routes!(get_version))
        .routes(routes!(delete_version))
        .routes(routes!(download_artifact_blob))
        .routes(routes!(upload_artifact))
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
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<ArtifactListParams>,
) -> Result<Json<Page<ArtifactSummaryResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Artifact, Action::Read)
        .await?;
    let page = state
        .spectra()
        .artifacts()
        .list_artifacts(params.q.as_deref(), &params.to_query())
        .await?;
    Ok(Json(ArtifactSummaryResponse::page(page)))
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
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(key): Path<ArtifactKey>,
) -> Result<Json<ArtifactSummaryResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Artifact, Action::Read)
        .await?;
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
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(key): Path<ArtifactKey>,
    Query(params): Query<VersionListParams>,
) -> Result<Json<Page<ArtifactVersionResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Artifact, Action::Read)
        .await?;
    let page = state
        .spectra()
        .artifacts()
        .list_versions(
            &key,
            params.sort(),
            params.media_type.as_ref(),
            params.version.as_deref(),
            &params.to_query(),
        )
        .await?;
    Ok(Json(ArtifactVersionResponse::page(page)))
}

/// Returns one stored version.
#[utoipa::path(
    get,
    path = "/{key}/versions/{digest}",
    operation_id = operation_ids::GET_ARTIFACT_VERSION,
    params(
        ("key" = ArtifactKey, Path, description = "Artifact key"),
        ("digest" = Digest, Path, description = "Content digest"),
    ),
    responses(
        (status = 200, description = "Version", body = ArtifactVersionResponse),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn get_version(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path((key, digest)): Path<(ArtifactKey, Digest)>,
) -> Result<Json<ArtifactVersionResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Artifact, Action::Read)
        .await?;
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
        ("digest" = Digest, Path, description = "Content digest"),
    ),
    responses(
        (status = 204, description = "Version evicted"),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn delete_version(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path((key, digest)): Path<(ArtifactKey, Digest)>,
) -> Result<StatusCode, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Artifact, Action::Evict)
        .await?;
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
/// The request body is stored as a content-addressed blob. Subsequent uploads
/// of identical bytes do not replace the prior version. Instead, a new
/// `(key, digest)` row is recorded when the digest differs. This matches the
/// content-addressed storage model but is not a strict RFC 9110 PUT (which
/// would replace prior state). Treat this endpoint as "store these bytes under
/// this key" rather than "set this key to these bytes".
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
        (status = 201, description = "Version stored", body = ArtifactVersionResponse,
         headers(
            ("Location" = String, description = "Path to the newly stored version"),
         )),
        (status = 413, description = "Body exceeded the maximum upload size"),
    ),
)]
async fn upload_artifact(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(key): Path<ArtifactKey>,
    headers: HeaderMap,
    request: Request,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<ArtifactVersionResponse>,
    ),
    ApiError,
> {
    state
        .auth()
        .require(&auth.subject, Object::Artifact, Action::Write)
        .await?;
    // Parse Content-Type as a MediaType at the boundary. An unparseable
    // value is treated as absent, so the store records no media type rather
    // than garbage.
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<MediaType>().ok());
    let stream = request
        .into_body()
        .into_data_stream()
        .map_err(io::Error::other);
    let reader = StreamReader::new(stream);
    let artifact = state
        .spectra()
        .artifacts()
        .put(&key, media_type.as_ref(), reader)
        .await?;
    // Artifact keys are URL-safe ([a-zA-Z0-9._-]) so they round-trip through
    // a single path segment without percent-encoding.
    let location = format!("/api/v1/artifacts/{key}/versions/{}", artifact.digest);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(artifact.into()),
    ))
}

/// Downloads the blob content of one stored version.
#[utoipa::path(
    get,
    path = "/{key}/versions/{digest}/blob",
    operation_id = operation_ids::DOWNLOAD_ARTIFACT_BLOB,
    params(
        ("key" = ArtifactKey, Path, description = "Artifact key"),
        ("digest" = Digest, Path, description = "Content digest"),
    ),
    responses(
        (status = 200, description = "Blob content",
         headers(
            ("ETag" = String, description = "Quoted content digest"),
            ("Last-Modified" = String, description = "HTTP timestamp of the upload"),
            ("Cache-Control" = String, description = "Immutable caching directive"),
            ("X-Content-Type-Options" = String, description = "Always `nosniff`"),
         )),
        (status = 304, description = "Not Modified"),
        (status = 404, description = "Unknown version"),
    ),
)]
async fn download_artifact_blob(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path((key, digest)): Path<(ArtifactKey, Digest)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Artifact, Action::Download)
        .await?;
    let artifacts = state.spectra().artifacts();
    let artifact = artifacts
        .version(&key, &digest)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;

    let etag = Etag::from_digest(&artifact.digest);
    let last_modified = HttpDate::from_timestamp(artifact.downloaded_at);

    if etag.is_not_modified(&headers, artifact.downloaded_at) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag.into()),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
                (header::LAST_MODIFIED, last_modified.as_header()),
            ],
        )
            .into_response());
    }

    let located = artifacts
        .locate_version(&key, &digest)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    let file = File::open(&located.path).await.map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            ApiError::NOT_FOUND
        } else {
            ArtifactError::from(err).into()
        }
    })?;
    let content_type = artifact.media_type.clone().unwrap_or_else(|| {
        "application/octet-stream"
            .parse()
            .expect("valid media type")
    });

    Response::builder()
        .header(header::CONTENT_TYPE, content_type.to_string())
        .header(header::CONTENT_LENGTH, artifact.size_bytes)
        .header(header::ETAG, HeaderValue::from(etag))
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::LAST_MODIFIED, last_modified.as_header())
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(ApiError::from)
}
