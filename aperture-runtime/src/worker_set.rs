//! [`WorkerSet`]: spawn a batch of named tasks, await any exit, drain with
//! timeout.

use std::error::Error as StdError;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::timeout;

/// A collection of spawned tasks tracked together for shutdown.
///
/// The set is the single source of truth for what is running. A supervisor
/// uses it to:
///
/// - spawn workers under a shared name,
/// - detect the first worker that exits early (so a single failure tears the
///   rest down instead of leaving them running headless), and
/// - drain every worker with a hard timeout on shutdown.
///
/// `WorkerSet` does not own a cancellation token. The supervisor is
/// responsible for distributing stop signals; the set just tracks join
/// handles and reports on their state.
pub struct WorkerSet {
    handles: FuturesUnordered<NamedHandle>,
    pending: Vec<NamedHandle>,
}

struct NamedHandle {
    name: &'static str,
    handle: JoinHandle<()>,
}

impl Future for NamedHandle {
    type Output = (&'static str, Result<(), JoinError>);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let name = self.name;
        let pinned: Pin<&mut JoinHandle<()>> = Pin::new(&mut self.handle);
        match pinned.poll(cx) {
            Poll::Ready(result) => Poll::Ready((name, result)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl WorkerSet {
    /// Creates an empty set.
    pub fn new() -> Self {
        Self {
            handles: FuturesUnordered::new(),
            pending: Vec::new(),
        }
    }

    /// Spawns `fut` as a named task. The name surfaces in drain logs.
    pub fn spawn<F>(&mut self, name: &'static str, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(fut);
        // Keep new handles in `pending` so `wait_for_any_exit` can move them
        // into the FuturesUnordered in one batch before polling. This avoids
        // a borrow of `self` across the await below.
        self.pending.push(NamedHandle { name, handle });
    }

    /// Returns `true` if no tasks have been spawned.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.handles.is_empty()
    }

    /// Polls until at least one task exits.
    ///
    /// Returns the name of the exited task, or `None` when every task has
    /// finished. Panicked tasks are logged at `error` level but treated as a
    /// normal exit (the name is still returned so the caller can react).
    pub async fn wait_for_any_exit(&mut self) -> Option<&'static str> {
        self.flush_pending();
        if self.handles.is_empty() {
            return None;
        }
        let (name, result) = self.handles.next().await?;
        if let Err(err) = result {
            tracing::error!(
                worker = name,
                error = &err as &dyn StdError,
                "worker exited unexpectedly"
            );
        }
        Some(name)
    }

    /// Awaits every still-running task with a hard `deadline`.
    ///
    /// Tasks that finish before the deadline are awaited normally. Tasks that
    /// do not finish are detached (the `WorkerSet` is dropped mid-iteration)
    /// and left for the runtime to clean up at process exit. Panics are
    /// logged at `error` level.
    pub async fn drain(self, deadline: Duration) {
        match timeout(deadline, self.drain_all()).await {
            Ok(()) => tracing::info!("worker set drain complete"),
            Err(_) => tracing::warn!(
                "worker set drain timed out after {deadline:?}, detaching remaining tasks"
            ),
        }
    }

    async fn drain_all(mut self) {
        self.flush_pending();
        while let Some((name, result)) = self.handles.next().await {
            if let Err(err) = result {
                tracing::error!(
                    worker = name,
                    error = &err as &dyn StdError,
                    "worker panicked during drain"
                );
            }
        }
    }

    fn flush_pending(&mut self) {
        let pending = mem::take(&mut self.pending);
        for h in pending {
            self.handles.push(h);
        }
    }
}

impl Default for WorkerSet {
    fn default() -> Self {
        Self::new()
    }
}
