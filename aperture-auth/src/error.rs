//! Errors returned by the auth layer.

use std::error::Error as StdError;
use std::result::Result as StdResult;

use argon2::password_hash;

/// Errors from the auth layer.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("database error: {0}")]
    Storage(#[from] aperture_storage::StorageError),

    #[error("policy error: {0}")]
    Policy(#[source] anyhow::Error),

    #[error("password hash error: {0}")]
    PasswordHash(#[source] anyhow::Error),

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

impl AuthError {
    /// Converts a casbin error into an [`AuthError`]. If the casbin error
    /// wraps a [`StorageError`] (the common case, since our adapter boxes
    /// storage errors into casbin's `AdapterError`), the original storage
    /// error is recovered so callers see the real cause. Everything else is
    /// wrapped opaquely as [`AuthError::Policy`].
    pub(crate) fn from_casbin(err: casbin::Error) -> Self {
        if let casbin::Error::AdapterError(adapter_err) = err {
            let boxed: Box<dyn StdError + Send + Sync> = adapter_err.0;
            return match boxed.downcast::<aperture_storage::StorageError>() {
                Ok(storage_err) => AuthError::Storage(*storage_err),
                Err(other) => AuthError::Policy(anyhow::Error::from_boxed(other)),
            };
        }
        AuthError::Policy(err.into())
    }

    /// Converts a password hashing error into an [`AuthError`] without
    /// leaking the underlying crate type.
    pub(crate) fn from_password_hash(err: password_hash::Error) -> Self {
        AuthError::PasswordHash(anyhow::Error::msg(err))
    }
}

pub type Result<T> = StdResult<T, AuthError>;
