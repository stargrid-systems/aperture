//! [`Supervisor`]: owns a set of [`Worker`]s, drives them with a shared stop
//! signal, and drains them in registration order on shutdown.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::Worker;
use crate::worker_set::WorkerSet;

/// Maximum time the supervisor waits for a worker to drain before detaching
/// it. Per-worker graceful shutdown should finish well within this; the
/// ceiling exists so a single stuck worker cannot hang process exit.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Stop signal handed to a [`Worker`]. Resolves when the supervisor asks for
/// a graceful shutdown.
pub type Stop = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Owns a set of [`Worker`]s and orchestrates their shutdown.
///
/// Workers are spawned in registration order. On shutdown they are stopped
/// (their `stop` future fires) and drained in the same order, with a hard
/// timeout per worker.
///
/// If a worker exits before the supervisor's signal, the supervisor still
/// drains the remaining workers but the operator can interrupt a slow drain
/// with a second signal.
pub struct Supervisor {
    workers: WorkerSet,
    triggers: Vec<oneshot::Sender<()>>,
}

impl Supervisor {
    /// Creates an empty supervisor.
    pub fn new() -> Self {
        Self {
            workers: WorkerSet::new(),
            triggers: Vec::new(),
        }
    }

    /// Spawns `worker` under `name`. The worker receives a `stop` future that
    /// resolves when [`Supervisor::trigger`] is called or the supervisor is
    /// dropped.
    pub fn spawn<W: Worker>(&mut self, name: &'static str, worker: W) {
        let (tx, rx) = oneshot::channel();
        self.triggers.push(tx);
        let stop: Stop = Box::pin(async move {
            let _ = rx.await;
        });
        let run = worker.run(stop);
        self.workers.spawn(name, run);
    }

    /// Fires every worker's stop signal. Workers that have already exited are
    /// unaffected.
    pub fn trigger(&mut self) {
        for tx in self.triggers.drain(..) {
            let _ = tx.send(());
        }
    }

    /// Runs until either `signal` resolves or every worker exits on its own.
    ///
    /// On `signal`, drains workers in registration order with a per-worker
    /// timeout. On early worker exit, drains the remainder with an option for
    /// the operator to interrupt with a second signal.
    pub async fn run_until_signal<F>(mut self, signal: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // Bridge the abstract `signal` future into a CancellationToken so we
        // can poll the "is the signal triggered" state more than once. The
        // raw future would panic if polled after completion.
        let signal_token = CancellationToken::new();
        let signal_watcher = {
            let token = signal_token.clone();
            tokio::spawn(async move {
                signal.await;
                token.cancel();
            })
        };

        // Wait for either the signal to fire or any worker to exit.
        let first_signal = tokio::select! {
            biased;
            _ = signal_token.cancelled() => true,
            name = self.workers.wait_for_any_exit() => {
                if let Some(name) = name {
                    tracing::info!(
                        worker = name,
                        "worker exited early, draining remaining workers"
                    );
                }
                false
            }
        };

        self.trigger();
        let deadline = DRAIN_TIMEOUT;
        if first_signal {
            self.workers.drain(deadline).await;
        } else {
            // Early exit: drain with a second-signal escape hatch.
            tokio::select! {
                _ = self.workers.drain(deadline) => {}
                _ = signal_token.cancelled() => {
                    tracing::warn!("second shutdown signal received, forcing exit");
                }
            }
        }
        signal_watcher.abort();
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}
