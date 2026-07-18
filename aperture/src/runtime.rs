//! Worker trait and supervisor for composing long-running background tasks.
//!
//! A [`Worker`] is anything with a `run` method that takes a `stop` future and
//! drains before returning. The [`Supervisor`] owns the join handles and stop
//! triggers for a set of workers, so a single OS signal can drain them all in
//! registration order.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use aperture_http::HttpServer;
use aperture_tasks::{Scheduler, Tasks};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the scheduler wakes to check for due schedules.
const SCHEDULER_TICK: Duration = Duration::from_secs(60);

pub(crate) type Stop = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub(crate) trait Worker: Sized + Send + 'static {
    fn run(self, stop: Stop) -> impl Future<Output = ()> + Send;
}

pub(crate) struct Supervisor {
    handles: Vec<(&'static str, JoinHandle<()>)>,
    triggers: Vec<oneshot::Sender<()>>,
}

impl Supervisor {
    pub(crate) fn new() -> Self {
        Self {
            handles: Vec::new(),
            triggers: Vec::new(),
        }
    }

    pub(crate) fn spawn<W>(&mut self, name: &'static str, worker: W)
    where
        W: Worker,
    {
        let (tx, rx) = oneshot::channel();
        self.triggers.push(tx);
        let stop: Stop = Box::pin(async move {
            let _ = rx.await;
        });
        let handle = tokio::spawn(worker.run(stop));
        self.handles.push((name, handle));
    }

    pub(crate) fn trigger(&mut self) {
        for tx in self.triggers.drain(..) {
            let _ = tx.send(());
        }
    }

    pub(crate) async fn await_all(self) {
        for (name, handle) in self.handles {
            match timeout(DRAIN_TIMEOUT, handle).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::error!(worker = name, error = %err, "worker panicked"),
                Err(_) => tracing::error!(worker = name, "worker did not drain within 30s"),
            }
        }
    }

    pub(crate) async fn run_until_signal<F>(mut self, signal: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let signal = tokio::spawn(signal);
        let early_exit = async {
            for (name, handle) in &mut self.handles {
                if let Err(err) = handle.await {
                    tracing::error!(worker = name, error = %err, "worker exited unexpectedly");
                    return;
                }
            }
        };
        tokio::select! {
            biased;
            _ = signal => {}
            _ = early_exit => {}
        }
        self.trigger();
        self.await_all().await;
    }
}

/// Adapter so [`HttpServer`] fits the [`Worker`] trait.
pub(crate) struct HttpServerWorker(pub(crate) HttpServer);

impl Worker for HttpServerWorker {
    async fn run(self, stop: Stop) {
        self.0.run(stop).await;
    }
}

pub(crate) struct TasksWorker {
    scheduler: Scheduler,
    tasks: Tasks,
}

impl TasksWorker {
    pub(crate) fn new(scheduler: Scheduler, tasks: Tasks) -> Self {
        Self { scheduler, tasks }
    }
}

impl Worker for TasksWorker {
    async fn run(self, stop: Stop) {
        if let Err(err) = self.tasks.reconcile().await {
            tracing::error!(error = %err, "tasks reconciliation failed");
        }
        self.scheduler.run(SCHEDULER_TICK, stop).await;
        self.tasks.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::sync::Mutex;
    use tokio::time::sleep;

    use super::*;

    struct Counter {
        started: Arc<AtomicUsize>,
        stopped: Arc<AtomicUsize>,
    }

    impl Worker for Counter {
        async fn run(self, stop: Stop) {
            self.started.fetch_add(1, Ordering::SeqCst);
            stop.await;
            self.stopped.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn trigger_fires_all_workers() {
        let started = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicUsize::new(0));

        let mut supervisor = Supervisor::new();
        for _ in 0..3 {
            supervisor.spawn(
                "counter",
                Counter {
                    started: Arc::clone(&started),
                    stopped: Arc::clone(&stopped),
                },
            );
        }

        // Let the spawned tasks reach their first await point.
        sleep(Duration::from_millis(20)).await;
        assert_eq!(started.load(Ordering::SeqCst), 3);
        assert_eq!(stopped.load(Ordering::SeqCst), 0);

        supervisor.trigger();
        supervisor.await_all().await;

        assert_eq!(stopped.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn await_all_respects_registration_order() {
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut supervisor = Supervisor::new();
        for i in 0..5 {
            let order = Arc::clone(&order);
            supervisor.spawn("ordered", Stamped { i, order });
        }

        supervisor.trigger();
        supervisor.await_all().await;

        let recorded = order.lock().await.clone();
        assert_eq!(recorded, vec![0, 1, 2, 3, 4]);
    }

    struct Stamped {
        i: usize,
        order: Arc<Mutex<Vec<usize>>>,
    }

    impl Worker for Stamped {
        async fn run(self, stop: Stop) {
            stop.await;
            self.order.lock().await.push(self.i);
        }
    }

    #[tokio::test]
    async fn run_until_signal_drains_on_signal() {
        let started = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicUsize::new(0));

        let mut supervisor = Supervisor::new();
        supervisor.spawn(
            "counter",
            Counter {
                started: Arc::clone(&started),
                stopped: Arc::clone(&stopped),
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

        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }
}
