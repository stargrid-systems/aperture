//! Maps storage, artifact-manager, task, and auth errors onto HTTP status
//! codes.

use std::error::Error as StdError;

use aperture_artifacts::ArtifactError;
use aperture_auth::AuthError;
use aperture_settings::SettingError;
use aperture_storage::StorageError;
use aperture_tasks::TaskError;
use axum::http::{Error as HttpError, StatusCode};
use axum::response::{IntoResponse, Response};
use http_body_util::LengthLimitError;

/// An error turned into an HTTP response.
///
/// Server faults are logged.
pub struct ApiError(StatusCode);

impl ApiError {
    /// The request was malformed.
    pub(crate) const BAD_REQUEST: Self = Self(StatusCode::BAD_REQUEST);
    /// Authentication is required but was not provided.
    pub(crate) const UNAUTHORIZED: Self = Self(StatusCode::UNAUTHORIZED);
    /// The authenticated actor lacks permission.
    pub(crate) const FORBIDDEN: Self = Self(StatusCode::FORBIDDEN);
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
            TaskError::NotRegistered(_)
            | TaskError::DecodeInput(_)
            | TaskError::Storage(StorageError::InvalidCursor(_)) => StatusCode::BAD_REQUEST,
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

impl From<SettingError> for ApiError {
    fn from(err: SettingError) -> Self {
        let status = match &err {
            SettingError::NotRegistered(_) => StatusCode::NOT_FOUND,
            SettingError::Decode(_) => StatusCode::BAD_REQUEST,
            SettingError::Storage(_) | SettingError::Encode(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn StdError, "setting request failed");
        }
        Self(status)
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        let status = match &err {
            AuthError::InvalidCredentials
            | AuthError::SessionNotFound
            | AuthError::ApiKeyNotFound => StatusCode::UNAUTHORIZED,
            AuthError::PasswordTooShort(_)
            | AuthError::PasswordTooLong(_)
            | AuthError::PasswordReuse
            | AuthError::InvalidUsername => StatusCode::BAD_REQUEST,
            AuthError::ActorDisabled | AuthError::MustChangePassword | AuthError::Forbidden => {
                StatusCode::FORBIDDEN
            }
            AuthError::CannotDeleteSelf | AuthError::LastAdmin => StatusCode::CONFLICT,
            AuthError::TooManyAttempts => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = &err as &dyn StdError, "auth request failed");
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
    let mut current: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(e) = current {
        if e.is::<LengthLimitError>() {
            return true;
        }
        current = e.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::io;

    use aperture_artifacts::ArtifactError;
    use axum::Router;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use futures_util::TryStreamExt as _;
    use tokio::io::AsyncReadExt;
    use tokio_util::io::StreamReader;
    use tower::ServiceExt;
    use tower_http::limit::RequestBodyLimitLayer;

    use super::{ApiError, chain_contains_length_limit};

    #[test]
    fn unrelated_io_error_is_not_a_length_limit() {
        assert!(!chain_contains_length_limit(&io::Error::other("boom")));
    }

    // Reproduces the upload router's body pipeline (RequestBodyLimitLayer +
    // into_data_stream + map_err(io::Error::other)) with a tiny limit, then
    // confirms the genuine limit error maps to 413 through the real
    // From<ArtifactError> conversion. tower-http's limit wrapper surfaces the
    // LengthLimitError as a source node, unlike a plain Box<dyn Error>, so the
    // pipeline must be exercised end to end rather than hand-built.
    #[tokio::test]
    async fn oversized_upload_maps_to_payload_too_large() {
        async fn upload(request: Request) -> Response {
            let stream = request
                .into_body()
                .into_data_stream()
                .map_err(io::Error::other);
            let mut reader = StreamReader::new(stream);
            let mut buf = Vec::new();
            match reader.read_to_end(&mut buf).await {
                Ok(_) => StatusCode::OK.into_response(),
                Err(err) => ApiError::from(ArtifactError::from(err)).into_response(),
            }
        }

        let app = Router::new()
            .route("/", post(upload))
            .layer(RequestBodyLimitLayer::new(8));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/")
                    .body(Body::from(vec![0u8; 64]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
