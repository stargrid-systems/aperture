//! Error type for the task system.

use aperture_storage::StorageError;

/// Errors returned by the task system.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    /// No definition is registered for the requested kind.
    #[error("no task definition registered for kind {0:?}")]
    NotRegistered(String),
    /// The task input could not be decoded into the kind's input type.
    #[error("failed to decode task input")]
    DecodeInput(#[source] serde_json::Error),
    /// The task output could not be decoded into the kind's output type.
    #[error("failed to decode task output")]
    DecodeOutput(#[source] serde_json::Error),
    /// The task input could not be encoded to JSON.
    #[error("failed to encode task input")]
    EncodeInput(#[source] serde_json::Error),
    /// The task output could not be encoded to JSON.
    #[error("failed to encode task output")]
    EncodeOutput(#[source] serde_json::Error),
    /// A storage operation failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// The task was cancelled before it finished.
    #[error("task was cancelled")]
    Cancelled,
    /// The referenced task is not currently running.
    #[error("task {0} is not running")]
    NotRunning(i64),
    /// The referenced task does not exist.
    #[error("task {0} not found")]
    NotFound(i64),
    /// The task body failed.
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}
