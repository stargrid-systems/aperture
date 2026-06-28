//! Error types for the task system.
//!
//! Two concerns are kept apart. [`RunError`] is what a task body returns from
//! its `run` function: the body was cancelled or it failed. [`TaskError`] is the
//! system error for operating the task system: resolving kinds, encoding and
//! decoding payloads, and talking to storage.

use aperture_storage::StorageError;

/// The outcome of a task body that did not succeed.
///
/// A task author returns this from `TaskDefinition::run`. Any error converts into
/// [`RunError::Failed`] with `?`, and a cooperative stop is reported as
/// [`RunError::Cancelled`].
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The task observed cancellation and stopped at a safe point.
    #[error("task was cancelled")]
    Cancelled,
    /// The task body failed.
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

/// Errors from operating the task system: resolving a kind, moving payloads
/// across the typed boundary, and recording invocations.
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
    /// The referenced task is not currently running.
    #[error("task {0} is not running")]
    NotRunning(i64),
    /// The referenced task does not exist.
    #[error("task {0} not found")]
    NotFound(i64),
    /// The task body did not succeed.
    #[error(transparent)]
    Run(#[from] RunError),
}

impl From<TaskError> for RunError {
    /// Lets a task body use `?` on task-system calls (spawning a child, awaiting
    /// its output). A nested body outcome is preserved, so a cancelled child
    /// surfaces as [`RunError::Cancelled`]. Any other system error becomes a
    /// [`RunError::Failed`].
    fn from(err: TaskError) -> Self {
        match err {
            TaskError::Run(run) => run,
            other => RunError::Failed(other.into()),
        }
    }
}
