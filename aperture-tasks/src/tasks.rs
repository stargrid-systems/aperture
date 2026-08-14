//! [`Tasks`]: spawns tasks, tracks the running ones, and records every
//! invocation in storage.
//!
//! The durable record of every invocation lives in the storage catalog. A live
//! registry, held here, tracks only the tasks running right now, with their
//! cancellation token, progress, and abort handle. Spawning runs the body on a
//! [`JoinSet`] and returns a typed [`TaskHandle`].

use std::collections::HashMap;
use std::error::Error;
use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use aperture_storage::{
    ActorId, JsonFilter, ListQuery, Page, ParentFilter, StatusFilter, TaskId, TaskInvocation,
    TaskRepository, TaskStatus,
};
use jiff::Timestamp;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::watch;
use tokio::task::{AbortHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::context::TaskContext;
use crate::definition::{Capabilities, TaskDefinition};
use crate::error::{RunError, TaskError};
use crate::progress::{Progress, ProgressState};
use crate::registry::TaskRegistry;

/// Whether a tracked task is still running or has settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Running,
    Settled,
}

/// State shared between a running task's body, its live-registry entry, and any
/// handle observing it.
struct TaskShared {
    cancel: CancellationToken,
    progress: Arc<ProgressState>,
    phase: watch::Sender<Phase>,
}

/// A task currently being tracked in the live registry.
struct RunningTask {
    shared: Arc<TaskShared>,
    kind: String,
    parent_id: Option<TaskId>,
    capabilities: Capabilities,
    started_at: Timestamp,
    abort: AbortHandle,
}

/// A snapshot of one running task, for display.
#[derive(Debug, Clone)]
pub struct ActiveTask {
    /// The invocation id.
    pub id: TaskId,
    /// The kind of task.
    pub kind: String,
    /// The parent invocation, if any.
    pub parent_id: Option<TaskId>,
    /// What the kind supports.
    pub capabilities: Capabilities,
    /// Live progress.
    pub progress: Progress,
    /// When the task started.
    pub started_at: Timestamp,
}

pub struct TasksInner {
    tasks: TaskRepository,
    registry: TaskRegistry,
    running: Mutex<HashMap<TaskId, RunningTask>>,
    joinset: Mutex<JoinSet<()>>,
}

/// Spawns and tracks tasks. Cheap to clone: all clones share one instance.
#[derive(Clone)]
pub struct Tasks {
    inner: Arc<TasksInner>,
}

impl Tasks {
    /// Creates a task runtime backed by `tasks` repository and the kinds in
    /// `registry`.
    pub fn new(tasks: TaskRepository, registry: TaskRegistry) -> Self {
        Self {
            inner: Arc::new(TasksInner {
                tasks,
                registry,
                running: Mutex::new(HashMap::new()),
                joinset: Mutex::new(JoinSet::new()),
            }),
        }
    }

    /// The registry of kinds, for projecting schemas and capabilities.
    pub fn registry(&self) -> &TaskRegistry {
        &self.inner.registry
    }

    /// Spawns a top-level task of kind `T` and returns a typed handle to it.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::EncodeInput` if the input cannot be encoded, or a
    /// storage or kind-resolution error from the spawn.
    pub async fn spawn<T: TaskDefinition>(
        &self,
        input: T::Input,
        initiator: ActorId,
    ) -> Result<TaskHandle<T::Output>, TaskError> {
        let value = serde_json::to_value(input).map_err(TaskError::EncodeInput)?;
        self.inner
            .spawn_value::<T::Output>(T::KEY, value, None, initiator)
            .await
    }

    /// Spawns a top-level task by kind string, validating `input` against the
    /// kind's input type, and returns the created invocation. Used by the API,
    /// which does not await a typed output.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::NotRegistered` if `kind` is unknown,
    /// `TaskError::DecodeInput` if the input fails validation, or a storage
    /// error if recording the invocation fails.
    pub async fn create(
        &self,
        kind: &str,
        input: Value,
        initiator: ActorId,
    ) -> Result<TaskInvocation, TaskError> {
        let (invocation, _phase) = self.inner.start(kind, input, None, initiator).await?;
        Ok(invocation)
    }

    /// Lists recorded invocations, optionally filtered by status, kind, parent,
    /// and any number of `json` field matches over the input/output payloads.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::Storage` if the query fails.
    pub async fn list(
        &self,
        status: Option<StatusFilter>,
        kind: Option<&str>,
        parent: Option<ParentFilter>,
        json: &[JsonFilter<'_>],
        query: &ListQuery,
    ) -> Result<Page<TaskInvocation>, TaskError> {
        Ok(self
            .inner
            .tasks
            .list(status, kind, parent, json, query)
            .await?)
    }

    /// Returns the recorded invocation `id`, if it exists.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::Storage` if the lookup fails.
    pub async fn get(&self, id: TaskId) -> Result<Option<TaskInvocation>, TaskError> {
        Ok(self.inner.tasks.get(id).await?)
    }

    /// Requests cooperative cancellation of the running task `id`. Returns
    /// `true` if cancellation was requested and `false` if the kind is not
    /// cancellable. Returns [`TaskError::AlreadySettled`] if the task
    /// exists but has finished, and [`TaskError::NotFound`] if no such task
    /// exists.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::AlreadySettled` if the task already finished, or
    /// `TaskError::NotFound` if no such task exists.
    ///
    /// # Panics
    ///
    /// Panics if the running-task registry lock is poisoned.
    pub async fn cancel(&self, id: TaskId) -> Result<bool, TaskError> {
        {
            let running = self.inner.running.lock().expect("running poisoned");
            if let Some(task) = running.get(&id) {
                if !task.capabilities.cancellable {
                    return Ok(false);
                }
                task.shared.cancel.cancel();
                return Ok(true);
            }
        }
        // Not running: tell an unknown id apart from one that already finished.
        match self.inner.tasks.get(id).await? {
            Some(_) => Err(TaskError::AlreadySettled(id)),
            None => Err(TaskError::NotFound(id)),
        }
    }

    /// A snapshot of every task running right now.
    ///
    /// # Panics
    ///
    /// Panics if the running-task registry lock is poisoned.
    pub fn active(&self) -> Vec<ActiveTask> {
        let running = self.inner.running.lock().expect("running poisoned");
        running
            .iter()
            .map(|(id, task)| ActiveTask {
                id: *id,
                kind: task.kind.clone(),
                parent_id: task.parent_id,
                capabilities: task.capabilities,
                progress: task.shared.progress.snapshot(),
                started_at: task.started_at,
            })
            .collect()
    }

    /// Live progress of the running task `id`, or `None` if it is not running.
    ///
    /// # Panics
    ///
    /// Panics if the running-task registry lock is poisoned.
    pub fn progress(&self, id: TaskId) -> Option<Progress> {
        let running = self.inner.running.lock().expect("running poisoned");
        running.get(&id).map(|task| task.shared.progress.snapshot())
    }

    /// Marks invocations left active by a previous process as interrupted.
    /// Call once at startup, before spawning anything. Returns how many were
    /// reconciled.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::Storage` if listing active tasks or recording the
    /// interrupted status fails.
    pub async fn reconcile(&self) -> Result<usize, TaskError> {
        let now = Timestamp::now();
        let mut count = 0;
        for task in self.inner.tasks.list_active().await? {
            self.inner
                .tasks
                .finish(
                    task.id,
                    TaskStatus::Interrupted,
                    now,
                    None,
                    Some("interrupted"),
                )
                .await?;
            count += 1;
        }
        Ok(count)
    }

    /// Stops accepting nothing new here, but resolves the running set for
    /// shutdown: resumable tasks are aborted and recorded as interrupted, while
    /// unresumable tasks are awaited so they finish cleanly.
    ///
    /// # Panics
    ///
    /// Panics if the running-task registry lock is poisoned.
    pub async fn shutdown(&self) {
        let entries: Vec<(TaskId, bool, AbortHandle, Arc<TaskShared>)> = {
            let running = self.inner.running.lock().expect("running poisoned");
            running
                .iter()
                .map(|(id, task)| {
                    (
                        *id,
                        task.capabilities.resumable,
                        task.abort.clone(),
                        Arc::clone(&task.shared),
                    )
                })
                .collect()
        };

        let mut awaiting = Vec::new();
        for (id, resumable, abort, shared) in entries {
            if resumable {
                abort.abort();
                let now = Timestamp::now();
                if let Err(err) = self
                    .inner
                    .tasks
                    .finish(id, TaskStatus::Interrupted, now, None, Some("interrupted"))
                    .await
                {
                    tracing::error!(
                        task = id.get(),
                        error = &err as &dyn Error,
                        "failed to record interrupted task"
                    );
                }
                self.inner.settle(id);
            } else {
                awaiting.push(shared.phase.subscribe());
            }
        }

        for mut phase in awaiting {
            while *phase.borrow_and_update() == Phase::Running {
                if phase.changed().await.is_err() {
                    break;
                }
            }
        }
    }
}

impl TasksInner {
    /// Creates the invocation already running and spawns the body. The live
    /// entry, including its abort handle, is inserted before the body can
    /// settle, so a fast completion always finds it and shutdown can always
    /// abort it. Returns the created invocation and a receiver of its
    /// completion phase.
    pub(crate) async fn start(
        self: &Arc<Self>,
        kind: &str,
        input: Value,
        parent_id: Option<TaskId>,
        initiator: ActorId,
    ) -> Result<(TaskInvocation, watch::Receiver<Phase>), TaskError> {
        let definition = Arc::clone(
            self.registry
                .get(kind)
                .ok_or_else(|| TaskError::NotRegistered(kind.to_owned()))?,
        );
        definition.validate(&input)?;

        let now = Timestamp::now();
        let id = self
            .tasks
            .create_running(kind, parent_id, initiator, &input, now)
            .await?;

        let cancel = parent_id
            .and_then(|parent| self.parent_token(parent))
            .map_or_else(CancellationToken::new, |parent| parent.child_token());
        let (phase_tx, phase_rx) = watch::channel(Phase::Running);
        let shared = Arc::new(TaskShared {
            cancel: cancel.clone(),
            progress: Arc::new(ProgressState::default()),
            phase: phase_tx,
        });

        let ctx = TaskContext::new(
            id,
            initiator,
            Arc::clone(self),
            cancel,
            Arc::clone(&shared.progress),
        );
        let capabilities = definition.capabilities();

        // Hold the registry lock across spawn and insert so the body cannot
        // settle (and try to remove the entry) before it exists, and so the
        // abort handle is stored before anyone can observe the task.
        {
            let mut running = self.running.lock().expect("running poisoned");
            let abort = {
                let mut set = self.joinset.lock().expect("joinset poisoned");
                definition.spawn_on(input.clone(), ctx, &mut set)
            };
            running.insert(
                id,
                RunningTask {
                    shared: Arc::clone(&shared),
                    kind: kind.to_owned(),
                    parent_id,
                    capabilities,
                    started_at: now,
                    abort,
                },
            );
        }

        let invocation = TaskInvocation {
            id,
            kind: kind.to_owned(),
            parent_id,
            initiator_id: initiator,
            status: TaskStatus::Running,
            input,
            output: None,
            error: None,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
        };
        Ok((invocation, phase_rx))
    }

    /// Spawns a task and returns a typed handle to its output.
    pub(crate) async fn spawn_value<O>(
        self: &Arc<Self>,
        kind: &str,
        input: Value,
        parent_id: Option<TaskId>,
        initiator: ActorId,
    ) -> Result<TaskHandle<O>, TaskError> {
        let (invocation, phase) = self.start(kind, input, parent_id, initiator).await?;
        Ok(TaskHandle {
            id: invocation.id,
            inner: Arc::clone(self),
            phase,
            _output: PhantomData,
        })
    }

    fn parent_token(&self, parent: TaskId) -> Option<CancellationToken> {
        self.running
            .lock()
            .expect("running poisoned")
            .get(&parent)
            .map(|task| task.shared.cancel.clone())
    }

    /// Records the terminal outcome of `id` and wakes anyone awaiting it.
    pub(crate) async fn finish(&self, id: TaskId, outcome: Result<Value, TaskError>) {
        let now = Timestamp::now();
        let (status, output, error) = match outcome {
            Ok(value) => (TaskStatus::Succeeded, Some(value), None),
            Err(TaskError::Run(RunError::Cancelled)) => (TaskStatus::Cancelled, None, None),
            // `{:#}` keeps the full source chain, not just the outermost message.
            Err(err) => (TaskStatus::Failed, None, Some(format!("{err:#}"))),
        };
        match self
            .tasks
            .finish(id, status, now, output.as_ref(), error.as_deref())
            .await
        {
            Ok(()) => {}
            Err(err) => {
                tracing::error!(
                    task = id.get(),
                    error = &err as &dyn Error,
                    "failed to record task outcome"
                );
            }
        }
        self.settle(id);
    }

    /// Removes `id` from the live registry and signals its completion.
    ///
    /// Also reaps any completed entries from the `JoinSet`. Tokio's `JoinSet`
    /// retains finished entries until they are joined, so without this the
    /// set would grow without bound over the process lifetime.
    fn settle(&self, id: TaskId) {
        let removed = self.running.lock().expect("running poisoned").remove(&id);
        if let Some(task) = removed {
            let _ = task.shared.phase.send(Phase::Settled);
        }
        // Reap completed JoinSet entries. Tokio's JoinSet retains finished
        // entries until they are joined, so without this the set would grow
        // without bound. The entry for `id` itself is not reaped here because
        // `settle` runs from within that task's body; it is collected on a
        // later call.
        loop {
            let next = self
                .joinset
                .lock()
                .expect("joinset poisoned")
                .try_join_next();
            match next {
                Some(Ok(())) => {}
                Some(Err(err)) => {
                    tracing::error!(error = &err as &dyn Error, "task panicked");
                }
                None => break,
            }
        }
    }
}

/// A typed handle to a spawned task. Await it for the output, or read live
/// [`TaskHandle::progress`] while it runs.
pub struct TaskHandle<O> {
    id: TaskId,
    inner: Arc<TasksInner>,
    phase: watch::Receiver<Phase>,
    _output: PhantomData<fn() -> O>,
}

impl<O> TaskHandle<O> {
    /// The invocation id.
    pub const fn id(&self) -> TaskId {
        self.id
    }

    /// Live progress, or `None` once the task has settled.
    ///
    /// # Panics
    ///
    /// Panics if the running-task registry lock is poisoned.
    pub fn progress(&self) -> Option<Progress> {
        self.inner
            .running
            .lock()
            .expect("running poisoned")
            .get(&self.id)
            .map(|task| task.shared.progress.snapshot())
    }
}

impl<O: DeserializeOwned> TaskHandle<O> {
    /// Waits for the task to settle and returns its decoded output.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::NotFound` if the invocation vanished,
    /// `TaskError::DecodeOutput` if the output cannot be decoded, or
    /// `TaskError::Run` if the task failed or was cancelled.
    pub async fn wait(mut self) -> Result<O, TaskError> {
        while *self.phase.borrow_and_update() == Phase::Running {
            if self.phase.changed().await.is_err() {
                break;
            }
        }

        let task = self
            .inner
            .tasks
            .get(self.id)
            .await?
            .ok_or(TaskError::NotFound(self.id))?;
        match task.status {
            TaskStatus::Succeeded => {
                let output = task.output.ok_or_else(|| {
                    TaskError::Run(RunError::Failed(anyhow::format_err!(
                        "task {} succeeded without output",
                        self.id
                    )))
                })?;
                serde_json::from_value(output).map_err(TaskError::DecodeOutput)
            }
            TaskStatus::Cancelled => Err(TaskError::Run(RunError::Cancelled)),
            TaskStatus::Failed | TaskStatus::Interrupted => Err(TaskError::Run(RunError::Failed(
                anyhow::format_err!("{}", task.error.unwrap_or_else(|| "task failed".to_owned())),
            ))),
            TaskStatus::Pending | TaskStatus::Running => Err(TaskError::Run(RunError::Failed(
                anyhow::format_err!("task {} still active after settle", self.id),
            ))),
        }
    }
}

impl<O: DeserializeOwned + Send + 'static> IntoFuture for TaskHandle<O> {
    type Output = Result<O, TaskError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Result<O, TaskError>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.wait())
    }
}
