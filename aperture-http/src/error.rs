//! Maps storage, artifact-manager, and task errors onto HTTP status codes.

use std::error::Error;

use aperture_artifacts::ArtifactError;
use aperture_storage::StorageError;
use aperture_tasks::TaskError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// An error turned into an HTTP response. Server faults are logged.
pub(crate) struct ApiError(StatusCode);

impl ApiError {
    /// The request was malformed.
    pub(crate) const BAD_REQUEST: Self = Self(StatusCode::BAD_REQUEST);
    /// The requested resource does not exist.
    pub(crate) const NOT_FOUND: Self = Self(StatusCode::NOT_FOUND);
    /// The request conflicts with the resource's current state.
    pub(crate) const CONFLICT: Self = Self(StatusCode::CONFLICT);
}

impl From<ArtifactError> for ApiError {
    fn from(err: ArtifactError) -> Self {
        let status = match &err {
            ArtifactError::Storage(StorageError::InvalidCursor(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn Error, "artifact request failed");
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
            tracing::error!(error = &err as &dyn Error, "storage request failed");
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
            tracing::error!(error = &err as &dyn Error, "task request failed");
        }
        Self(status)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}
