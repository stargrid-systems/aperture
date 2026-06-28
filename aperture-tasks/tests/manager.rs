use std::sync::{Arc, Mutex};

use aperture_storage::{Storage, TaskStatus};
use aperture_tasks::{Capabilities, TaskContext, TaskDefinition, TaskError, TaskManager, TaskRegistry};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use utoipa::ToSchema;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap()
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
    const KIND: &'static str = "double";
    type Input = DoubleIn;
    type Output = DoubleOut;

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    async fn run(&self, input: DoubleIn, _ctx: TaskContext) -> Result<DoubleOut, TaskError> {
        Ok(DoubleOut {
            result: input.n * 2,
        })
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
    const KIND: &'static str = "probe";
    type Input = Empty;
    type Output = Empty;

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancellable: self.cancellable,
            resumable: false,
        }
    }

    async fn run(&self, _input: Empty, ctx: TaskContext) -> Result<Empty, TaskError> {
        let progress = ctx.progress();
        progress.set_total(10);
        progress.set_done(4);
        progress.set_message("working");
        self.ready.notify_one();
        tokio::select! {
            _ = self.gate.notified() => Ok(Empty {}),
            _ = ctx.cancellation_token().cancelled() => Err(TaskError::Cancelled),
        }
    }
}

/// A task that spawns a [`Probe`] child, publishes the child's id, then awaits
/// it. When the parent is cancelled the child is too, so the await unwinds.
struct Parent {
    child_id: Arc<Mutex<Option<i64>>>,
    spawned: Arc<Notify>,
}

impl TaskDefinition for Parent {
    const KIND: &'static str = "parent";
    type Input = Empty;
    type Output = Empty;

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancellable: true,
            resumable: false,
        }
    }

    async fn run(&self, _input: Empty, ctx: TaskContext) -> Result<Empty, TaskError> {
        let child = ctx.spawn_child::<Probe>(Empty {}).await?;
        *self.child_id.lock().unwrap() = Some(child.id());
        self.spawned.notify_one();
        child.wait().await?;
        Ok(Empty {})
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
    let manager = TaskManager::new(storage.clone(), registry);

    let handle = manager.spawn::<Double>(DoubleIn { n: 21 }).await.unwrap();
    let id = handle.id();
    let output = handle.wait().await.unwrap();
    assert_eq!(output.result, 42);

    let recorded = storage.tasks().get(id).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Succeeded);
    assert_eq!(recorded.output.as_deref(), Some(r#"{"result":42}"#));
}

#[tokio::test]
async fn live_progress_is_visible_while_running() {
    let storage = Storage::open(":memory:").await.unwrap();
    let (probe, ready, gate) = probe(false);
    let mut registry = TaskRegistry::new();
    registry.register(probe);
    let manager = TaskManager::new(storage, registry);

    let handle = manager.spawn::<Probe>(Empty {}).await.unwrap();
    ready.notified().await;

    let progress = handle.progress().expect("running task has progress");
    assert_eq!(progress.total, Some(10));
    assert_eq!(progress.done, Some(4));
    assert_eq!(progress.message.as_deref(), Some("working"));

    gate.notify_one();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn cancellable_task_records_cancelled() {
    let storage = Storage::open(":memory:").await.unwrap();
    let (probe, ready, _gate) = probe(true);
    let mut registry = TaskRegistry::new();
    registry.register(probe);
    let manager = TaskManager::new(storage.clone(), registry);

    let handle = manager.spawn::<Probe>(Empty {}).await.unwrap();
    let id = handle.id();
    ready.notified().await;

    assert!(manager.cancel(id).unwrap());
    let result = handle.wait().await;
    assert!(matches!(result, Err(TaskError::Cancelled)));

    let recorded = storage.tasks().get(id).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn cancel_is_refused_for_non_cancellable_kind() {
    let storage = Storage::open(":memory:").await.unwrap();
    let (probe, ready, gate) = probe(false);
    let mut registry = TaskRegistry::new();
    registry.register(probe);
    let manager = TaskManager::new(storage, registry);

    let handle = manager.spawn::<Probe>(Empty {}).await.unwrap();
    let id = handle.id();
    ready.notified().await;

    assert!(!manager.cancel(id).unwrap());

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
    let manager = TaskManager::new(storage.clone(), registry);

    let handle = manager.spawn::<Parent>(Empty {}).await.unwrap();
    let parent_id = handle.id();
    spawned.notified().await;
    child_ready.notified().await;
    let child = child_id.lock().unwrap().expect("child id published");

    // Cancelling the parent cancels the child through the shared token.
    assert!(manager.cancel(parent_id).unwrap());
    assert!(matches!(handle.wait().await, Err(TaskError::Cancelled)));

    let recorded = storage.tasks().get(child).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Cancelled);
    assert_eq!(recorded.parent_id, Some(parent_id));
}

#[tokio::test]
async fn reconcile_marks_orphaned_invocations() {
    let storage = Storage::open(":memory:").await.unwrap();
    let id = storage
        .tasks()
        .create("double", None, "{}", at(1_000))
        .await
        .unwrap();
    storage.tasks().mark_running(id, at(1_000)).await.unwrap();

    let manager = TaskManager::new(storage.clone(), TaskRegistry::new());
    assert_eq!(manager.reconcile().await.unwrap(), 1);

    let recorded = storage.tasks().get(id).await.unwrap().unwrap();
    assert_eq!(recorded.status, TaskStatus::Interrupted);
}
