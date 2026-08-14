use std::future::{Future, ready};
use std::sync::{Arc, Mutex};

use aperture_storage::{ActorId, Storage, TaskId, TaskStatus};
use aperture_tasks::{
    Capabilities, ProgressMessage, RunError, TaskContext, TaskDefinition, TaskError, TaskRegistry,
    Tasks,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use utoipa::ToSchema;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

#[derive(Serialize, Deserialize, ToSchema)]
struct DoubleIn {
    n: u64,
}

#[derive(Serialize, Deserialize, ToSchema)]
struct DoubleOut {
    result: u64,
}

/// A task that doubles its input and finishes immediately.
struct Double;

impl TaskDefinition for Double {
    const KEY: &'static str = "double";
    type Input = DoubleIn;
    type Output = DoubleOut;

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    fn run(
        &self,
        input: DoubleIn,
        _ctx: TaskContext,
    ) -> impl Future<Output = Result<DoubleOut, RunError>> + Send {
        ready(Ok(DoubleOut {
            result: input.n * 2,
        }))
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
struct Empty {}

/// A task that reports progress, signals `ready`, then waits for either `gate`
/// or cancellation. Lets a test observe a task while it runs.
struct Probe {
    cancellable: bool,
    ready: Arc<Notify>,
    gate: Arc<Notify>,
}

impl TaskDefinition for Probe {
    const KEY: &'static str = "probe";
    type Input = Empty;
    type Output = Empty;

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancellable: self.cancellable,
            resumable: false,
        }
    }

    async fn run(&self, _input: Empty, ctx: TaskContext) -> Result<Empty, RunError> {
        let progress = ctx.progress();
        progress.set_total(10);
        progress.set_done(4);
        progress.set_message(ProgressMessage::new("working"));
        self.ready.notify_one();
        tokio::select! {
            () = self.gate.notified() => Ok(Empty {}),
            () = ctx.cancellation_token().cancelled() => Err(RunError::Cancelled),
        }
    }
}

/// A task that spawns a [`Probe`] child, publishes the child's id, then awaits
/// it. When the parent is cancelled the child is too, so the await unwinds.
struct Parent {
    child_id: Arc<Mutex<Option<TaskId>>>,
    spawned: Arc<Notify>,
}

impl TaskDefinition for Parent {
    const KEY: &'static str = "parent";
    type Input = Empty;
    type Output = Empty;

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancellable: true,
            resumable: false,
        }
    }

    async fn run(&self, _input: Empty, ctx: TaskContext) -> Result<Empty, RunError> {
        let child = ctx.spawn_child::<Probe>(Empty {}).await?;
        *self.child_id.lock().unwrap() = Some(child.id());
        self.spawned.notify_one();
        child.wait().await?;
        Ok(Empty {})
    }
}

/// A task whose body panics. Used to check a panic still settles the task.
struct Boom;

impl TaskDefinition for Boom {
    const KEY: &'static str = "boom";
    type Input = Empty;
    type Output = Empty;

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    async fn run(&self, _input: Empty, _ctx: TaskContext) -> Result<Empty, RunError> {
        panic!("boom");
    }
}

/// A task that fails with a layered error, to check the source chain is kept.
struct Fail;

impl TaskDefinition for Fail {
    const KEY: &'static str = "fail";
    type Input = Empty;
    type Output = Empty;

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    fn run(
        &self,
        _input: Empty,
        _ctx: TaskContext,
    ) -> impl Future<Output = Result<Empty, RunError>> + Send {
        ready(Err(RunError::Failed(
            anyhow::format_err!("root cause")
                .context("middle")
                .context("outer failure"),
        )))
    }
}

fn probe(cancellable: bool) -> (Probe, Arc<Notify>, Arc<Notify>) {
    let ready = Arc::new(Notify::new());
    let gate = Arc::new(Notify::new());
    let probe = Probe {
        cancellable,
        ready: Arc::clone(&ready),
        gate: Arc::clone(&gate),
    };
    (probe, ready, gate)
}

#[tokio::test]
async fn spawn_and_wait_returns_decoded_output() {
    let storage = Storage::open(":memory:").await.unwrap();
    let mut registry = TaskRegistry::new();
    registry.register(Double);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let handle = tasks
        .spawn::<Double>(DoubleIn { n: 21 }, ActorId::SYSTEM)
        .await
        .unwrap();
    let id = handle.id();
    let output = handle.wait().await.unwrap();
    assert_eq!(output.result, 42);

    let recorded = storage.tasks().unwrap().get(id).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Succeeded);
    assert_eq!(recorded.output, Some(serde_json::json!({"result": 42})));
}

#[tokio::test]
async fn live_progress_is_visible_while_running() {
    let storage = Storage::open(":memory:").await.unwrap();
    let (probe, ready, gate) = probe(false);
    let mut registry = TaskRegistry::new();
    registry.register(probe);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let handle = tasks
        .spawn::<Probe>(Empty {}, ActorId::SYSTEM)
        .await
        .unwrap();
    ready.notified().await;

    let progress = handle.progress().expect("running task has progress");
    assert_eq!(progress.total, Some(10));
    assert_eq!(progress.done, Some(4));
    assert_eq!(
        progress.message.expect("running task has a message").key,
        "working"
    );

    gate.notify_one();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn cancellable_task_records_cancelled() {
    let storage = Storage::open(":memory:").await.unwrap();
    let (probe, ready, _gate) = probe(true);
    let mut registry = TaskRegistry::new();
    registry.register(probe);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let handle = tasks
        .spawn::<Probe>(Empty {}, ActorId::SYSTEM)
        .await
        .unwrap();
    let id = handle.id();
    ready.notified().await;

    assert!(tasks.cancel(id).await.unwrap());
    let result = handle.wait().await;
    assert!(matches!(result, Err(TaskError::Run(RunError::Cancelled))));

    let recorded = storage.tasks().unwrap().get(id).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn cancel_is_refused_for_non_cancellable_kind() {
    let storage = Storage::open(":memory:").await.unwrap();
    let (probe, ready, gate) = probe(false);
    let mut registry = TaskRegistry::new();
    registry.register(probe);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let handle = tasks
        .spawn::<Probe>(Empty {}, ActorId::SYSTEM)
        .await
        .unwrap();
    let id = handle.id();
    ready.notified().await;

    assert!(!tasks.cancel(id).await.unwrap());

    gate.notify_one();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn child_inherits_parent_cancellation() {
    let storage = Storage::open(":memory:").await.unwrap();
    let (probe, child_ready, _gate) = probe(true);
    let child_id = Arc::new(Mutex::new(None));
    let spawned = Arc::new(Notify::new());
    let parent = Parent {
        child_id: Arc::clone(&child_id),
        spawned: Arc::clone(&spawned),
    };
    let mut registry = TaskRegistry::new();
    registry.register(probe);
    registry.register(parent);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let handle = tasks
        .spawn::<Parent>(Empty {}, ActorId::SYSTEM)
        .await
        .unwrap();
    let parent_id = handle.id();
    spawned.notified().await;
    child_ready.notified().await;
    let child = child_id.lock().unwrap().expect("child id published");

    // Cancelling the parent cancels the child through the shared token.
    assert!(tasks.cancel(parent_id).await.unwrap());
    assert!(matches!(
        handle.wait().await,
        Err(TaskError::Run(RunError::Cancelled))
    ));

    let recorded = storage.tasks().unwrap().get(child).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Cancelled);
    assert_eq!(recorded.parent_id, Some(parent_id));
}

#[tokio::test]
async fn panicking_task_settles_as_failed() {
    let storage = Storage::open(":memory:").await.unwrap();
    let mut registry = TaskRegistry::new();
    registry.register(Boom);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let handle = tasks
        .spawn::<Boom>(Empty {}, ActorId::SYSTEM)
        .await
        .unwrap();
    let id = handle.id();

    // The body panics, but wait() must still return rather than hang forever.
    assert!(matches!(
        handle.wait().await,
        Err(TaskError::Run(RunError::Failed(_)))
    ));

    let recorded = storage.tasks().unwrap().get(id).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Failed);

    // A panicked task settles, so shutdown finds nothing to wait on.
    tasks.shutdown().await;
}

#[tokio::test]
async fn failed_task_records_full_error_chain() {
    let storage = Storage::open(":memory:").await.unwrap();
    let mut registry = TaskRegistry::new();
    registry.register(Fail);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let handle = tasks
        .spawn::<Fail>(Empty {}, ActorId::SYSTEM)
        .await
        .unwrap();
    let id = handle.id();
    assert!(matches!(
        handle.wait().await,
        Err(TaskError::Run(RunError::Failed(_)))
    ));

    let recorded = storage.tasks().unwrap().get(id).await.unwrap().unwrap();
    let error = recorded.error.expect("failed task records an error");
    assert!(error.contains("outer failure"), "{error:?}");
    assert!(error.contains("root cause"), "{error:?}");
}

#[tokio::test]
async fn cancel_distinguishes_unknown_from_settled() {
    let storage = Storage::open(":memory:").await.unwrap();
    let mut registry = TaskRegistry::new();
    registry.register(Double);
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    // An unknown id is not found.
    let unknown = TaskId::from(999);
    assert!(matches!(
        tasks.cancel(unknown).await,
        Err(TaskError::NotFound(id)) if id == unknown
    ));

    // A task that already finished is reported as settled, not unknown.
    let handle = tasks
        .spawn::<Double>(DoubleIn { n: 1 }, ActorId::SYSTEM)
        .await
        .unwrap();
    let id = handle.id();
    handle.wait().await.unwrap();
    assert!(
        matches!(tasks.cancel(id).await, Err(TaskError::AlreadySettled(settled)) if settled == id)
    );
}

#[tokio::test]
async fn reconcile_marks_orphaned_invocations() {
    let storage = Storage::open(":memory:").await.unwrap();
    let id = storage
        .tasks()
        .unwrap()
        .create(
            "double",
            None,
            ActorId::SYSTEM,
            &serde_json::json!({}),
            at(1_000),
        )
        .await
        .unwrap();
    storage
        .tasks()
        .unwrap()
        .mark_running(id, at(1_000))
        .await
        .unwrap();

    let tasks = Tasks::new(storage.tasks().unwrap(), TaskRegistry::new());
    assert_eq!(tasks.reconcile().await.unwrap(), 1);

    let recorded = storage.tasks().unwrap().get(id).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Interrupted);
}
