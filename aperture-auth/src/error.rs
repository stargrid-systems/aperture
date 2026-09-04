//! Errors returned by the auth layer.

use std::result::Result as StdResult;

/// Errors from the auth layer.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("database error: {0}")]
    Storage(#[from] aperture_storage::StorageError),

    #[error("internal error")]
    Internal(#[source] anyhow::Error),

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("password must be at least {0} characters")]
    PasswordTooShort(usize),

    #[error("password must be at most {0} characters")]
    PasswordTooLong(usize),

    #[error("new password must differ from the current password")]
    PasswordReuse,

    #[error("invalid username")]
    InvalidUsername,

    #[error("unknown role: {0}")]
    UnknownRole(String),

    #[error("session not found")]
    SessionNotFound,

    #[error("api key not found")]
    ApiKeyNotFound,

    #[error("actor is disabled")]
    ActorDisabled,

    #[error("password must be changed before continuing")]
    MustChangePassword,

    #[error("permission denied")]
    Forbidden,

    #[error("cannot delete yourself")]
    CannotDeleteSelf,

    #[error("cannot remove the last admin")]
    LastAdmin,

    #[error("too many login attempts, try again later")]
    TooManyAttempts,
}

pub type Result<T> = StdResult<T, AuthError>;
