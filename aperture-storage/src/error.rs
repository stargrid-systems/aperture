//! Error type for the storage layer.

use std::result::Result as StdResult;

use crate::digest::InvalidDigest;
use crate::key::InvalidArtifactKey;

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

    /// The database stored an unknown actor kind string.
    #[error("unknown actor kind {0:?}")]
    UnknownActorKind(String),

    /// The database stored an unknown policy type string.
    #[error("unknown policy type {0:?}")]
    UnknownPolicyType(String),

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

    /// An integer at the given column does not fit in the target type.
    #[error("integer {value} at column {column} does not fit in {target}")]
    IntegerCast {
        column: usize,
        value: i64,
        target: &'static str,
    },

    /// A timestamp stored as microseconds could not be converted.
    #[error("invalid timestamp {micros} us")]
    InvalidTimestamp { micros: i64 },

    #[error("invalid interval: {error}")]
    InvalidInterval { error: String },

    /// A JSON column value could not be deserialized.
    #[error("invalid JSON at column {column}: {error}")]
    InvalidJson { column: usize, error: String },

    /// An artifact key failed validation.
    #[error("invalid artifact key: {0}")]
    InvalidArtifactKey(#[from] InvalidArtifactKey),

    /// A digest string failed validation.
    #[error("invalid digest: {raw}")]
    InvalidDigest { raw: String },

    /// A media type string failed validation.
    #[error("invalid media type: {raw}")]
    InvalidMediaType { raw: String },
}

impl StorageError {
    /// Wraps a turso engine error as a [`StorageError::Database`].
    pub(crate) fn from_turso(error: turso::Error) -> Self {
        Self::Database(error.into())
    }
}

impl From<InvalidDigest> for StorageError {
    fn from(err: InvalidDigest) -> Self {
        Self::InvalidDigest { raw: err.0 }
    }
}

/// Result alias for the storage layer.
pub type Result<T> = StdResult<T, StorageError>;
