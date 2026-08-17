//! Runtime composition primitives.
//!
//! Aperture is built out of long-running background tasks (the HTTP server,
//! the task scheduler, the log worker). Each implements [`Worker`]. A
//! [`Supervisor`] owns a set of workers, drives them with a shared stop
//! signal, and drains them on shutdown.
//!
//! Workers that themselves own multiple subtasks (for example, the HTTP
//! server's listener + reload watcher) compose via [`WorkerSet`].
//!
//! [`Registry`] is a generic keyed map for type-erased definitions, and
//! [`json_schema`] turns a type's `OpenAPI` component into a standalone JSON
//! Schema document.

use std::future::Future;

pub use self::batching::{BatchSink, run_batched};
pub use self::registry::{
    InvalidCursor, Order, Registry, RegistryEntry, RegistryPage, RegistryQuery,
};
pub use self::schema::json_schema;
pub use self::supervisor::{ShutdownOutcome, Stop, Supervisor};
pub use self::worker_set::WorkerSet;

mod batching;
mod registry;
mod schema;
mod supervisor;
mod worker_set;

/// A long-running background task that drains before returning.
///
/// Implementations receive a `stop` token ([`Stop`]) that resolves when the
/// supervisor is asking for a graceful shutdown. The implementation decides
/// how to react (typically by propagating the signal to its subtasks and then
/// awaiting their drain).
pub trait Worker: Sized + Send + 'static {
    /// Runs until `stop` resolves, then drains and returns.
    fn run(self, stop: Stop) -> impl Future<Output = ()> + Send;
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::sync::oneshot;
    use tokio::time::sleep;

    use super::*;

    struct Counter {
        started: Arc<AtomicUsize>,
        stopped: Arc<AtomicUsize>,
    }

    impl Worker for Counter {
        async fn run(self, stop: Stop) {
            self.started.fetch_add(1, Ordering::SeqCst);
            stop.cancelled().await;
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
        // `run_until_signal` drains every worker. We feed it a never-firing
        // signal so the drain runs to completion.
        supervisor.run_until_signal(pending::<()>()).await;

        assert_eq!(stopped.load(Ordering::SeqCst), 3);
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
