//! A [`Supervisor`] owns three tiers of [`Worker`]s, drives them with shared
//! stop signals, and drains each tier on shutdown in tier order.

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

/// Why [`Supervisor::run_until_signal`] returned.
#[derive(Debug)]
pub enum ShutdownOutcome {
    /// The shutdown signal fired and both tiers drained.
    Signaled,
    /// A worker exited before the shutdown signal. The remaining workers
    /// were still drained.
    EarlyExit {
        /// Name of the worker that exited before the signal.
        worker: &'static str,
    },
    /// The operator forced the exit with a second signal while a worker
    /// was still draining. The remaining tasks were detached.
    Forced,
}

/// Owns three tiers of [`Worker`]s and orchestrates their shutdown.
///
/// Regular workers stop on the supervisor signal and drain concurrently with
/// a hard timeout. Last-tier workers ([`Supervisor::spawn_last`]) stop only
/// after the regular tier has fully drained, so they can collect what the
/// regular workers still produce. Final-tier workers
/// ([`Supervisor::spawn_final`]) stop only after the last tier has fully
/// drained. Use the later tiers for workers that drain a channel fed by
/// earlier workers, keeping a channel's producer and its final consumer in
/// adjacent tiers: the event recorder drains after the regular tier, and the
/// log worker drains after the recorder, so it still records the recorder's
/// final flush.
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
    final_workers: WorkerSet,
    final_stop_token: CancellationToken,
}

impl Supervisor {
    /// Creates an empty supervisor.
    pub fn new() -> Self {
        Self {
            workers: WorkerSet::new(),
            stop_token: CancellationToken::new(),
            late_workers: WorkerSet::new(),
            late_stop_token: CancellationToken::new(),
            final_workers: WorkerSet::new(),
            final_stop_token: CancellationToken::new(),
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
    /// channel fed by other workers (the event recorder).
    pub fn spawn_last<W: Worker>(&mut self, name: &'static str, worker: W) {
        let stop = self.late_stop_token.clone();
        let run = worker.run(stop);
        self.late_workers.spawn(name, run);
    }

    /// Spawns `worker` in the final shutdown tier.
    ///
    /// Final-tier workers are stopped only after every last-tier worker has
    /// stopped and drained. Use for the worker that must outlast all other
    /// drains because it persists their log output (the log worker).
    pub fn spawn_final<W: Worker>(&mut self, name: &'static str, worker: W) {
        let stop = self.final_stop_token.clone();
        let run = worker.run(stop);
        self.final_workers.spawn(name, run);
    }

    /// Fires the stop signal of every regular worker. Workers that have
    /// already exited are unaffected. Last-tier workers stop after the
    /// regular tier drains.
    pub fn trigger(&self) {
        self.stop_token.cancel();
    }

    /// Runs until either `signal` resolves or a worker exits on its own.
    ///
    /// Shutdown happens in tier order. The regular workers are stopped and
    /// drained concurrently with a hard timeout. Last-tier workers stop only
    /// after the regular tier has finished, final-tier workers only after
    /// the last tier, so each tier can collect what earlier tiers still
    /// produce.
    ///
    /// On `signal`, all tiers drain and the outcome is
    /// [`ShutdownOutcome::Signaled`]. On early worker exit, the outcome
    /// carries the worker's name and the operator can interrupt a slow
    /// drain with a second signal, which forces the exit, detaches any
    /// remaining tasks, and yields [`ShutdownOutcome::Forced`].
    pub async fn run_until_signal<F>(mut self, signal: F) -> ShutdownOutcome
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // Fuse the signal future so it can be polled in two consecutive
        // select! blocks without panicking after completion. Once it fires,
        // further polls return Pending forever.
        let mut signal = Box::pin(signal).fuse();
        let mut early_worker: Option<&'static str> = None;
        let mut forced = false;

        // Wait for either the signal to fire or any worker in any tier to
        // exit. The last and final tier branches are gated so an empty set
        // cannot win the biased select immediately, which would abort the
        // wait.
        tokio::select! {
            biased;
            () = &mut signal => {}
            name = self.workers.wait_for_any_exit() => {
                early_worker = name;
                if let Some(name) = name {
                    tracing::info!(
                        worker = name,
                        "worker exited early, draining remaining workers"
                    );
                }
            }
            name = self.late_workers.wait_for_any_exit(), if !self.late_workers.is_empty() => {
                early_worker = name;
                if let Some(name) = name {
                    tracing::info!(
                        worker = name,
                        "last-tier worker exited early, draining workers"
                    );
                }
            }
            name = self.final_workers.wait_for_any_exit(), if !self.final_workers.is_empty() => {
                early_worker = name;
                if let Some(name) = name {
                    tracing::info!(
                        worker = name,
                        "final-tier worker exited early, draining workers"
                    );
                }
            }
        }

        // Phase 1: stop the regular tier and drain it.
        self.trigger();
        let timeout = DRAIN_TIMEOUT;
        if early_worker.is_none() {
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

        // Phases 2 and 3: stop the last tier, drain it, then do the same
        // for the final tier. On a forced exit the remaining tasks detach
        // with the stuck regular tasks.
        self.late_stop_token.cancel();
        if !forced {
            self.late_workers.drain(timeout).await;
        }

        self.final_stop_token.cancel();
        if !forced {
            self.final_workers.drain(timeout).await;
        }

        if forced {
            ShutdownOutcome::Forced
        } else if let Some(worker) = early_worker {
            ShutdownOutcome::EarlyExit { worker }
        } else {
            ShutdownOutcome::Signaled
        }
    }

    /// Stops every tier and drains them in order: regular workers first,
    /// then last-tier workers, then final-tier workers.
    ///
    /// Used on the failed startup path, where no shutdown signal exists:
    /// workers already spawned must still drain so the records they hold
    /// reach storage before the error escapes. A tier's drain that exceeds
    /// the drain timeout detaches its remaining tasks.
    pub async fn shutdown(self) {
        self.trigger();
        self.workers.drain(DRAIN_TIMEOUT).await;
        self.late_stop_token.cancel();
        self.late_workers.drain(DRAIN_TIMEOUT).await;
        self.final_stop_token.cancel();
        self.final_workers.drain(DRAIN_TIMEOUT).await;
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
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

    /// A worker that returns immediately without waiting for the stop signal.
    struct Quitter;

    impl Worker for Quitter {
        async fn run(self, _stop: Stop) {}
    }

    /// A worker that ignores the stop signal and keeps running.
    struct Hung;

    impl Worker for Hung {
        async fn run(self, _stop: Stop) {
            sleep(Duration::from_secs(60)).await;
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
        let outcome = supervisor
            .run_until_signal(async {
                let _ = signal.await;
            })
            .await;
        assert!(matches!(outcome, ShutdownOutcome::Signaled));
        assert_eq!(*order.lock().unwrap(), vec!["regular", "late"]);
    }

    #[tokio::test]
    async fn final_tier_stops_after_last_tier_drains() {
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let mut supervisor = Supervisor::new();
        supervisor.spawn_last(
            "late",
            Recorder {
                tag: "late",
                delay: Duration::from_millis(50),
                order: Arc::clone(&order),
            },
        );
        supervisor.spawn_final(
            "final",
            Recorder {
                tag: "final",
                delay: Duration::ZERO,
                order: Arc::clone(&order),
            },
        );

        let (fire, signal) = oneshot::channel();
        tokio::spawn(async {
            sleep(Duration::from_millis(20)).await;
            let _ = fire.send(());
        });
        let outcome = supervisor
            .run_until_signal(async {
                let _ = signal.await;
            })
            .await;
        assert!(matches!(outcome, ShutdownOutcome::Signaled));
        assert_eq!(*order.lock().unwrap(), vec!["late", "final"]);
    }

    /// `shutdown` drains every tier in order without a shutdown signal. This
    /// is the failed-startup path.
    #[tokio::test]
    async fn shutdown_drains_all_tiers_without_a_signal() {
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let mut supervisor = Supervisor::new();
        supervisor.spawn(
            "regular",
            Recorder {
                tag: "regular",
                delay: Duration::ZERO,
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
        supervisor.spawn_final(
            "final",
            Recorder {
                tag: "final",
                delay: Duration::ZERO,
                order: Arc::clone(&order),
            },
        );

        supervisor.shutdown().await;
        assert_eq!(*order.lock().unwrap(), vec!["regular", "late", "final"]);
    }

    #[tokio::test]
    async fn reports_signaled_outcome_on_clean_shutdown() {
        let mut supervisor = Supervisor::new();
        supervisor.spawn(
            "regular",
            Recorder {
                tag: "regular",
                delay: Duration::ZERO,
                order: Arc::new(Mutex::new(Vec::new())),
            },
        );

        let (fire, signal) = oneshot::channel();
        tokio::spawn(async {
            sleep(Duration::from_millis(20)).await;
            let _ = fire.send(());
        });
        let outcome = supervisor
            .run_until_signal(async {
                let _ = signal.await;
            })
            .await;
        assert!(matches!(outcome, ShutdownOutcome::Signaled));
    }

    #[tokio::test]
    async fn reports_early_exit_with_worker_name() {
        let mut supervisor = Supervisor::new();
        supervisor.spawn("quitter", Quitter);
        supervisor.spawn(
            "regular",
            Recorder {
                tag: "regular",
                delay: Duration::ZERO,
                order: Arc::new(Mutex::new(Vec::new())),
            },
        );

        let outcome = supervisor.run_until_signal(pending::<()>()).await;
        match outcome {
            ShutdownOutcome::EarlyExit { worker } => assert_eq!(worker, "quitter"),
            other => panic!("expected an early exit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reports_forced_outcome_on_second_signal() {
        let mut supervisor = Supervisor::new();
        supervisor.spawn("quitter", Quitter);
        supervisor.spawn("hung", Hung);

        let (fire, signal) = oneshot::channel();
        tokio::spawn(async {
            sleep(Duration::from_millis(20)).await;
            let _ = fire.send(());
        });
        let outcome = supervisor
            .run_until_signal(async {
                let _ = signal.await;
            })
            .await;
        assert!(matches!(outcome, ShutdownOutcome::Forced));
    }
}
