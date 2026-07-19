//! Aperture-specific [`Worker`] implementations.
//!
//! The reusable supervisor primitives (worker trait, worker set, supervisor)
//! live in the `aperture-runtime` crate. This module wires the gateway's
//! concrete workers (the task scheduler, the log worker) into that machinery.
//! `HttpServer` already implements [`Worker`] inside `aperture-http`.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_runtime::{Stop, Worker};
use aperture_tasks::{Scheduler, Tasks};

/// How often the scheduler wakes to check for due schedules.
const SCHEDULER_TICK: Duration = Duration::from_secs(60);

/// Runs the periodic scheduler and owns the task runtime for its lifetime.
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
            tracing::error!(error = &err as &dyn StdError, "tasks reconciliation failed");
        }
        self.scheduler.run(SCHEDULER_TICK, stop).await;
        self.tasks.shutdown().await;
    }
}
