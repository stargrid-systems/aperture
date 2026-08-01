//! Errors returned by the auth layer.

use std::error::Error as StdError;
use std::result::Result as StdResult;

/// Errors from the auth layer.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("database error: {0}")]
    Storage(#[from] aperture_storage::StorageError),

    #[error("policy error: {0}")]
    Policy(#[source] anyhow::Error),

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
                Ok(storage_err) => Self::Storage(*storage_err),
                Err(other) => Self::Policy(anyhow::Error::from_boxed(other)),
            };
        }
        Self::Policy(err.into())
    }
}

pub type Result<T> = StdResult<T, AuthError>;
