//! Error type for the storage layer.

use std::result::Result as StdResult;

/// Errors returned by the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// An error from the underlying database engine. The engine type is hidden
    /// behind `anyhow::Error` so callers do not depend on it.
    #[error("database error: {0}")]
    Database(anyhow::Error),

    /// The stored schema version is newer than this binary supports.
    #[error("database schema version {current} is newer than the supported maximum {target}")]
    SchemaTooNew { current: i64, target: i64 },

    /// The stored schema version predates the baseline (squash) version.
    #[error("database schema version {current} is older than the baseline {baseline}")]
    SchemaTooOld { current: i64, baseline: i64 },

    /// `PRAGMA user_version` returned a non-integer value.
    #[error("user_version is not an integer: {value:?}")]
    InvalidUserVersion { value: turso::Value },

    /// The database stored an unknown log level integer.
    #[error("unknown log level {0}")]
    UnknownLogLevel(i64),

    /// The database stored an unknown task status string.
    #[error("unknown task status {0:?}")]
    UnknownTaskStatus(String),

    /// A cursor string from the client could not be decoded.
    #[error("invalid cursor: {0}")]
    InvalidCursor(String),

    /// A row column held a different turso value type than expected.
    #[error("expected {expected} at column {column}, found {actual:?}")]
    ColumnTypeMismatch {
        column: usize,
        expected: &'static str,
        actual: turso::Value,
    },

    /// An integer at the given column does not fit in `u32`.
    #[error("integer {value} at column {column} does not fit in u32")]
    U32OutOfRange { column: usize, value: i64 },

    /// A timestamp stored as milliseconds could not be converted.
    #[error("invalid timestamp {millis} ms")]
    InvalidTimestamp { millis: i64 },
}

/// Result alias for the storage layer.
pub type Result<T> = StdResult<T, StorageError>;

/// Wraps a turso engine error as a [`StorageError::Database`].
pub(crate) fn database(error: turso::Error) -> StorageError {
    StorageError::Database(error.into())
}
