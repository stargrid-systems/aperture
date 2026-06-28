//! The run context handed to a task body.

use std::sync::Arc;

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::definition::TaskDefinition;
use crate::error::TaskError;
use crate::manager::{ManagerInner, TaskHandle};
use crate::progress::{ProgressHandle, ProgressState};

/// Everything a running task needs: its identity, a cooperative cancellation
/// signal, a progress reporter, and the means to spawn sub-tasks under itself.
#[derive(Clone)]
pub struct TaskContext {
    id: i64,
    inner: Arc<ManagerInner>,
    cancel: CancellationToken,
    progress: Arc<ProgressState>,
}

impl TaskContext {
    pub(crate) fn new(
        id: i64,
        inner: Arc<ManagerInner>,
        cancel: CancellationToken,
        progress: Arc<ProgressState>,
    ) -> Self {
        Self {
            id,
            inner,
            cancel,
            progress,
        }
    }

    /// The id of the invocation this body is running.
    pub fn id(&self) -> i64 {
        self.id
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The cancellation token, for use in `tokio::select!` against long awaits.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancel
    }

    /// Returns [`TaskError::Cancelled`] if cancellation has been requested, so a
    /// body can bail at a safe point with `?`.
    pub fn check_cancelled(&self) -> Result<(), TaskError> {
        if self.cancel.is_cancelled() {
            Err(TaskError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// A handle to report this task's progress.
    pub fn progress(&self) -> ProgressHandle {
        ProgressHandle(Arc::clone(&self.progress))
    }

    /// Spawns a sub-task of kind `T`, recorded as a child of this invocation.
    /// The child's cancellation is tied to this task's, so cancelling the parent
    /// cancels the child.
    pub async fn spawn_child<T: TaskDefinition>(
        &self,
        input: T::Input,
    ) -> Result<TaskHandle<T::Output>, TaskError> {
        let value = serde_json::to_value(input).map_err(TaskError::EncodeInput)?;
        self.inner
            .spawn_value::<T::Output>(T::KIND, value, Some(self.id))
            .await
    }

    /// Records the task's terminal outcome and wakes anyone awaiting it.
    pub(crate) async fn complete(self, outcome: Result<Value, TaskError>) {
        self.inner.finish(self.id, outcome).await;
    }
}
