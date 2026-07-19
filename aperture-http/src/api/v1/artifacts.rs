//! Artifact catalog HTTP endpoints.
//!
//! # Trust boundary
//!
//! `PUT /api/v1/artifacts/{key}` accepts any well-formed `ArtifactKey`,
//! including the well-known `tls/*` keys the gateway uses for its own PKI.
//! Until authentication lands there is no namespace reservation: any caller
//! with network access can replace the CA private key and have the gateway
//! mint certs signed by it. Treat this endpoint as privileged. See the
//! `aperture_http::tls` module doc for the full threat model.

use std::io;

use aperture_artifacts::{ArtifactError, ArtifactKey};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::TryStreamExt as _;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use tokio::fs::File;
use tokio_util::io::{ReaderStream, StreamReader};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::conditional::{etag_from_digest, format_http_date, is_not_modified};
use crate::dto::{
    ArtifactListParams, ArtifactSummaryResponse, ArtifactVersionResponse, Page, VersionListParams,
    artifact_page, version_page,
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

/// ASCII set that percent-encodes everything unsafe inside a single URL path
/// segment. `/` is the most important one here, because artifact keys may
/// contain it (e.g. `tls/server-cert`) and an unencoded slash would shift the
/// route match.
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/');

pub fn router() -> OpenApiRouter<AppState> {
    use tower_http::limit::RequestBodyLimitLayer;
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
    // Artifacts::put validates the media type via MediaType::parse, so we just
    // forward the raw Content-Type header. An invalid value (e.g.
    // `text/html; charset=utf-8`) is silently dropped and stored as None.
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
    // Percent-encode the key so a multi-segment key like `tls/server-cert`
    // survives routing when the client follows the Location header.
    let encoded_key = utf8_percent_encode(key.as_str(), PATH_SEGMENT);
    let location = format!(
        "/api/v1/artifacts/{encoded_key}/versions/{}",
        artifact.digest
    );
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
        ("digest" = String, Path, description = "Content digest"),
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
    let last_modified = format_http_date(artifact.downloaded_at);

    if is_not_modified(&headers, &etag, artifact.downloaded_at) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
                (header::LAST_MODIFIED, last_modified),
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
    // The stored media type was validated at put time, so we can replay it
    // verbatim. The fallback keeps the safe default if the artifact has no
    // media type recorded.
    let content_type = artifact
        .media_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_owned());

    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, artifact.size_bytes)
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::LAST_MODIFIED, last_modified)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(ApiError::from)
}
