//! Maps storage, artifact-manager, and task errors onto HTTP status codes.

use std::error::Error as StdError;

use aperture_artifacts::ArtifactError;
use aperture_storage::StorageError;
use aperture_tasks::{SchedulerError, TaskError};
use axum::http::{Error as HttpError, StatusCode};
use axum::response::{IntoResponse, Response};

/// An error turned into an HTTP response.
///
/// Server faults are logged.
pub(crate) struct ApiError(StatusCode);

impl ApiError {
    /// The request was malformed.
    pub(crate) const BAD_REQUEST: Self = Self(StatusCode::BAD_REQUEST);
    /// The requested resource does not exist.
    pub(crate) const NOT_FOUND: Self = Self(StatusCode::NOT_FOUND);
    /// The request conflicts with the resource's current state.
    pub(crate) const CONFLICT: Self = Self(StatusCode::CONFLICT);
    /// The request body exceeded the advertised length limit.
    pub(crate) const PAYLOAD_TOO_LARGE: Self = Self(StatusCode::PAYLOAD_TOO_LARGE);
    /// Unexpected server-side failure.
    pub(crate) const INTERNAL_SERVER_ERROR: Self = Self(StatusCode::INTERNAL_SERVER_ERROR);
}

impl From<ArtifactError> for ApiError {
    fn from(err: ArtifactError) -> Self {
        if chain_contains_length_limit(&err) {
            return Self::PAYLOAD_TOO_LARGE;
        }
        let status = match &err {
            ArtifactError::Storage(StorageError::InvalidCursor(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn StdError, "artifact request failed");
        }
        Self(status)
    }
}

impl From<StorageError> for ApiError {
    fn from(err: StorageError) -> Self {
        let status = match &err {
            StorageError::InvalidCursor(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn StdError, "storage request failed");
        }
        Self(status)
    }
}

impl From<TaskError> for ApiError {
    fn from(err: TaskError) -> Self {
        let status = match &err {
            TaskError::NotRegistered(_) | TaskError::DecodeInput(_) => StatusCode::BAD_REQUEST,
            TaskError::Storage(StorageError::InvalidCursor(_)) => StatusCode::BAD_REQUEST,
            TaskError::NotFound(_) => StatusCode::NOT_FOUND,
            TaskError::AlreadySettled(_) => StatusCode::GONE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn StdError, "task request failed");
        }
        Self(status)
    }
}

impl From<SchedulerError> for ApiError {
    fn from(err: SchedulerError) -> Self {
        let status = match &err {
            SchedulerError::Storage(StorageError::InvalidCursor(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn StdError, "schedule request failed");
        }
        Self(status)
    }
}

impl From<HttpError> for ApiError {
    fn from(err: HttpError) -> Self {
        tracing::error!(error = &err as &dyn StdError, "response build failed");
        Self::INTERNAL_SERVER_ERROR
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

/// Walks the error source chain looking for a `LengthLimitError`.
///
/// When a client streams more bytes than `RequestBodyLimitLayer` allows without
/// declaring an oversized `Content-Length`, the limit error surfaces inside the
/// request body and is propagated through the upload pipeline as an
/// `io::Error`. Detect it here so we can answer with `413 Payload Too Large`
/// instead of a misleading `500`.
fn chain_contains_length_limit(err: &(dyn StdError + 'static)) -> bool {
    use http_body_util::LengthLimitError;

    let mut current: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(e) = current {
        if e.is::<LengthLimitError>() {
            return true;
        }
        current = e.source();
    }
    false
}
