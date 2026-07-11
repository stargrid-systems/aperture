//! Errors returned by the auth layer.

use argon2::password_hash;
use std::result::Result as StdResult;

/// Errors from the auth layer.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("database error: {0}")]
    Storage(#[from] aperture_storage::StorageError),

    #[error("casbin error: {0}")]
    Casbin(#[from] casbin::Error),

    #[error("password hash error: {0}")]
    PasswordHash(String),

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("session not found")]
    SessionNotFound,

    #[error("api key not found")]
    ApiKeyNotFound,

    #[error("actor is disabled")]
    ActorDisabled,

    #[error("password must be changed before continuing")]
    MustChangePassword,
}

impl From<password_hash::Error> for AuthError {
    fn from(err: password_hash::Error) -> Self {
        AuthError::PasswordHash(err.to_string())
    }
}

pub type Result<T> = StdResult<T, AuthError>;
