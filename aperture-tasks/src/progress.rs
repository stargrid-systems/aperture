//! Live progress reporting for running tasks.
//!
//! Progress is kept in memory only. A running task updates its counters through
//! a [`ProgressHandle`], and the manager reads a [`Progress`] snapshot for
//! display. Finished tasks have no live progress.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A snapshot of a task's progress. A missing `total` means the size is not
/// known, so the work is indeterminate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Progress {
    /// A short human-readable description of the current step.
    pub message: Option<String>,
    /// Units of work completed so far, once the task starts reporting.
    pub done: Option<u64>,
    /// Total units of work expected, if known.
    pub total: Option<u64>,
}

/// The shared, mutable progress counters behind a running task.
#[derive(Debug, Default)]
pub(crate) struct ProgressState {
    message: Mutex<Option<String>>,
    done: AtomicU64,
    total: AtomicU64,
    counting: AtomicBool,
}

impl ProgressState {
    pub(crate) fn snapshot(&self) -> Progress {
        Progress {
            message: self.message.lock().expect("progress poisoned").clone(),
            done: self
                .counting
                .load(Ordering::Relaxed)
                .then(|| self.done.load(Ordering::Relaxed)),
            total: match self.total.load(Ordering::Relaxed) {
                0 => None,
                total => Some(total),
            },
        }
    }
}

/// A handle a running task uses to report its progress.
#[derive(Clone)]
pub struct ProgressHandle(pub(crate) Arc<ProgressState>);

impl ProgressHandle {
    /// Sets the total units of work expected.
    pub fn set_total(&self, total: u64) {
        self.0.total.store(total, Ordering::Relaxed);
    }

    /// Sets the units of work completed so far.
    pub fn set_done(&self, done: u64) {
        self.0.done.store(done, Ordering::Relaxed);
        self.0.counting.store(true, Ordering::Relaxed);
    }

    /// Adds `n` units to the completed count.
    pub fn add(&self, n: u64) {
        self.0.done.fetch_add(n, Ordering::Relaxed);
        self.0.counting.store(true, Ordering::Relaxed);
    }

    /// Sets the current step description.
    pub fn set_message(&self, message: impl Into<String>) {
        *self.0.message.lock().expect("progress poisoned") = Some(message.into());
    }
}
