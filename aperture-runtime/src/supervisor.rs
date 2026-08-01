//! A [`Supervisor`] owns a set of [`Worker`]s, drives them with a shared stop
//! signal, and drains them on shutdown.

use std::future::Future;
use std::time::Duration;

use futures_util::future::FutureExt;
use tokio_util::sync::CancellationToken;

use crate::{Worker, WorkerSet};

/// Maximum time the supervisor waits for the drain to complete before
/// detaching any remaining tasks. Graceful shutdown should finish well within
/// this. The ceiling exists so a single stuck worker cannot hang process exit.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Stop signal handed to a [`Worker`]. A clone of the supervisor's
/// [`CancellationToken`]. Resolves when the supervisor asks for a graceful
/// shutdown.
pub type Stop = CancellationToken;

/// Owns a set of [`Worker`]s and orchestrates their shutdown.
///
/// On shutdown every worker's stop token fires and the workers drain
/// concurrently, with a shared hard timeout for the whole drain. A single
/// stuck worker cannot hang process exit: once the timeout elapses the
/// remaining tasks are detached.
///
/// If a worker exits before the supervisor's signal, the supervisor still
/// drains the remaining workers but the operator can interrupt a slow drain
/// with a second signal.
pub struct Supervisor {
    workers: WorkerSet,
    stop_token: CancellationToken,
}

impl Supervisor {
    /// Creates an empty supervisor.
    pub fn new() -> Self {
        Self {
            workers: WorkerSet::new(),
            stop_token: CancellationToken::new(),
        }
    }

    /// Spawns `worker` under `name`. The worker receives a [`Stop`] token
    /// that fires when [`Supervisor::trigger`] is called.
    pub fn spawn<W: Worker>(&mut self, name: &'static str, worker: W) {
        let stop = self.stop_token.clone();
        let run = worker.run(stop);
        self.workers.spawn(name, run);
    }

    /// Fires every worker's stop signal. Workers that have already exited are
    /// unaffected.
    pub fn trigger(&self) {
        self.stop_token.cancel();
    }

    /// Runs until either `signal` resolves or every worker exits on its own.
    ///
    /// On `signal`, drains the workers concurrently with a shared timeout.
    /// On early worker exit, drains the remainder with an option for the
    /// operator to interrupt with a second signal.
    pub async fn run_until_signal<F>(mut self, signal: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // Fuse the signal future so it can be polled in two consecutive
        // select! blocks without panicking after completion. Once it fires,
        // further polls return Pending forever.
        let mut signal = Box::pin(signal).fuse();
        let mut first_signal = false;

        // Wait for either the signal to fire or any worker to exit.
        tokio::select! {
            biased;
            () = &mut signal => {
                first_signal = true;
            }
            name = self.workers.wait_for_any_exit() => {
                if let Some(name) = name {
                    tracing::info!(
                        worker = name,
                        "worker exited early, draining remaining workers"
                    );
                }
            }
        }

        self.trigger();
        let timeout = DRAIN_TIMEOUT;
        if first_signal {
            self.workers.drain(timeout).await;
        } else {
            // Early exit: drain with a second-signal escape hatch.
            tokio::select! {
                () = self.workers.drain(timeout) => {}
                () = &mut signal => {
                    tracing::warn!("second shutdown signal received, forcing exit");
                }
            }
        }
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}
