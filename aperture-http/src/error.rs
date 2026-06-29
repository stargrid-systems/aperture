//! Maps storage and artifact-manager errors onto HTTP status codes.

use std::error::Error;

use aperture_artifacts::{ArtifactError, StorageError};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// An error turned into an HTTP response. Server faults are logged.
pub(crate) struct ApiError(StatusCode);

impl ApiError {
    /// The requested resource does not exist.
    pub(crate) const NOT_FOUND: Self = Self(StatusCode::NOT_FOUND);
    /// The request was malformed.
    pub(crate) const BAD_REQUEST: Self = Self(StatusCode::BAD_REQUEST);
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

impl From<StorageError> for ApiError {
    fn from(err: StorageError) -> Self {
        let status = match &err {
            StorageError::Decode(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn Error, "log request failed");
        }
        Self(status)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}
