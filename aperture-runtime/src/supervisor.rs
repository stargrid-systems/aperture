//! A [`Supervisor`] owns two tiers of [`Worker`]s, drives them with shared
//! stop signals, and drains each tier on shutdown.

use std::future::Future;
use std::time::Duration;

use futures_util::future::FutureExt;
use tokio_util::sync::CancellationToken;

use crate::{Worker, WorkerSet};

/// Maximum time the supervisor waits for a tier's drain to complete before
/// detaching any remaining tasks. Graceful shutdown should finish well within
/// this. The ceiling exists so a single stuck worker cannot hang process exit.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Stop signal handed to a [`Worker`]. A clone of the supervisor's
/// [`CancellationToken`]. Resolves when the supervisor asks for a graceful
/// shutdown.
pub type Stop = CancellationToken;

/// Owns two tiers of [`Worker`]s and orchestrates their shutdown.
///
/// Regular workers stop on the supervisor signal and drain concurrently with
/// a hard timeout. Last-tier workers ([`Supervisor::spawn_last`]) stop only
/// after the regular tier has fully drained, so they can collect what the
/// regular workers still produce. Use the last tier for workers that drain a
/// channel fed by other workers, such as the event recorder and the log
/// worker.
///
/// A single stuck worker cannot hang process exit. Once a tier's drain
/// timeout elapses, the remaining tasks in that tier are detached.
///
/// If a worker exits before the supervisor's signal, the supervisor still
/// drains the remaining workers but the operator can interrupt a slow drain
/// with a second signal.
pub struct Supervisor {
    workers: WorkerSet,
    stop_token: CancellationToken,
    late_workers: WorkerSet,
    late_stop_token: CancellationToken,
}

impl Supervisor {
    /// Creates an empty supervisor.
    pub fn new() -> Self {
        Self {
            workers: WorkerSet::new(),
            stop_token: CancellationToken::new(),
            late_workers: WorkerSet::new(),
            late_stop_token: CancellationToken::new(),
        }
    }

    /// Spawns `worker` under `name`. The worker receives a [`Stop`] token
    /// that fires when [`Supervisor::trigger`] is called.
    pub fn spawn<W: Worker>(&mut self, name: &'static str, worker: W) {
        let stop = self.stop_token.clone();
        let run = worker.run(stop);
        self.workers.spawn(name, run);
    }

    /// Spawns `worker` in the last shutdown tier.
    ///
    /// Last-tier workers are stopped only after every regular worker has
    /// stopped and drained, so they can collect what the regular workers
    /// still produce before shutting down. Use for workers that drain a
    /// channel fed by other workers (the event recorder, the log worker).
    pub fn spawn_last<W: Worker>(&mut self, name: &'static str, worker: W) {
        let stop = self.late_stop_token.clone();
        let run = worker.run(stop);
        self.late_workers.spawn(name, run);
    }

    /// Fires the stop signal of every regular worker. Workers that have
    /// already exited are unaffected. Last-tier workers stop after the
    /// regular tier drains.
    pub fn trigger(&self) {
        self.stop_token.cancel();
    }

    /// Runs until either `signal` resolves or every worker exits on its own.
    ///
    /// Shutdown happens in two phases. The regular workers are stopped and
    /// drain concurrently with a hard timeout. Last-tier workers stop only
    /// after the regular tier has finished, so they can collect what the
    /// regular workers still produce.
    ///
    /// On `signal`, both tiers drain. On early worker exit, the operator can
    /// interrupt a slow drain with a second signal, which forces exit and
    /// detaches any remaining tasks.
    pub async fn run_until_signal<F>(mut self, signal: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // Fuse the signal future so it can be polled in two consecutive
        // select! blocks without panicking after completion. Once it fires,
        // further polls return Pending forever.
        let mut signal = Box::pin(signal).fuse();
        let mut first_signal = false;
        let mut forced = false;

        // Wait for either the signal to fire or any worker in either tier to
        // exit. The last tier branch is gated so an empty set cannot win the
        // biased select immediately, which would abort the wait.
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
            name = self.late_workers.wait_for_any_exit(), if !self.late_workers.is_empty() => {
                if let Some(name) = name {
                    tracing::info!(
                        worker = name,
                        "last-tier worker exited early, draining workers"
                    );
                }
            }
        }

        // Phase 1: stop the regular tier and drain it.
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
                    forced = true;
                }
            }
        }

        // Phase 2: stop the last tier and drain it. On a forced exit the
        // last-tier tasks detach with the stuck regular tasks.
        self.late_stop_token.cancel();
        if !forced {
            self.late_workers.drain(timeout).await;
        }
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::oneshot;
    use tokio::time::sleep;

    use super::*;

    struct Recorder {
        tag: &'static str,
        delay: Duration,
        order: Arc<Mutex<Vec<&'static str>>>,
    }

    impl Worker for Recorder {
        async fn run(self, stop: Stop) {
            stop.cancelled().await;
            sleep(self.delay).await;
            self.order.lock().unwrap().push(self.tag);
        }
    }

    #[tokio::test]
    async fn last_tier_stops_after_regular_workers_drain() {
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let mut supervisor = Supervisor::new();
        supervisor.spawn(
            "regular",
            Recorder {
                tag: "regular",
                delay: Duration::from_millis(50),
                order: Arc::clone(&order),
            },
        );
        supervisor.spawn_last(
            "late",
            Recorder {
                tag: "late",
                delay: Duration::ZERO,
                order: Arc::clone(&order),
            },
        );

        let (fire, signal) = oneshot::channel();
        tokio::spawn(async {
            sleep(Duration::from_millis(20)).await;
            let _ = fire.send(());
        });
        supervisor
            .run_until_signal(async {
                let _ = signal.await;
            })
            .await;
        assert_eq!(*order.lock().unwrap(), vec!["regular", "late"]);
    }
}
