//! Maps artifact-manager errors onto HTTP status codes.

use std::error::Error;

use aperture_artifacts::{ArtifactError, StorageError};
use aperture_tasks::TaskError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// An error turned into an HTTP response. Server faults are logged.
pub(crate) struct ApiError(StatusCode);

impl ApiError {
    /// The requested resource does not exist.
    pub(crate) const NOT_FOUND: Self = Self(StatusCode::NOT_FOUND);
    /// The request conflicts with the resource's current state.
    pub(crate) const CONFLICT: Self = Self(StatusCode::CONFLICT);
}

impl From<ArtifactError> for ApiError {
    fn from(err: ArtifactError) -> Self {
        // A decode error means bad client input, most likely a malformed cursor.
        let status = match &err {
            ArtifactError::Storage(StorageError::Decode(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn Error, "artifact request failed");
        }
        Self(status)
    }
}

impl From<TaskError> for ApiError {
    fn from(err: TaskError) -> Self {
        let status = match &err {
            TaskError::NotRegistered(_) | TaskError::DecodeInput(_) => StatusCode::BAD_REQUEST,
            TaskError::Storage(StorageError::Decode(_)) => StatusCode::BAD_REQUEST,
            TaskError::NotFound(_) | TaskError::NotRunning(_) => StatusCode::NOT_FOUND,
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
