//! Error type for the storage layer.

use std::result::Result as StdResult;

/// Errors returned by the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// An error from the underlying database engine. The engine type is hidden
    /// behind `anyhow::Error` so callers do not depend on it.
    #[error("database error: {0}")]
    Database(anyhow::Error),
    /// A schema migration could not be applied.
    #[error("schema migration error: {0}")]
    Migration(String),
    /// Stored data could not be decoded into a domain type.
    #[error("failed to decode stored data: {0}")]
    Decode(String),
}

/// Result alias for the storage layer.
pub type Result<T> = StdResult<T, StorageError>;

/// Wraps a turso engine error as a [`StorageError::Database`].
pub(crate) fn database(error: turso::Error) -> StorageError {
    StorageError::Database(error.into())
}
