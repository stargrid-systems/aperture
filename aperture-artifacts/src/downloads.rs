//! In-memory tracking of ongoing downloads.
//!
//! The tracker coalesces concurrent requests for the same artifact onto a
//! single download (single-flight) and exposes live progress so the frontend
//! can show what is being fetched right now. The durable record of every
//! attempt lives in the storage catalog, not here.

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};

use jiff::Timestamp;
use tokio::io::AsyncWrite;
use tokio::sync::watch;

/// Running or terminal state of a single download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Running,
    Succeeded,
    Failed,
}

/// Byte counters for one download. `total` is `0` until the size is known.
#[derive(Debug, Default)]
pub(crate) struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    /// Records the expected total size, once it is known.
    pub(crate) fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// Adds `n` bytes to the transferred count.
    pub(crate) fn add(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    fn total(&self) -> Option<u64> {
        match self.total.load(Ordering::Relaxed) {
            0 => None,
            total => Some(total),
        }
    }
}

/// The shared state of one in-flight download. Held in the active map and by
/// every handle observing the download.
pub(crate) struct Slot {
    source: String,
    started_at: Timestamp,
    progress: Arc<Progress>,
    phase: watch::Sender<Phase>,
}

impl Slot {
    /// The byte counters to report transfer against.
    pub(crate) fn progress(&self) -> &Arc<Progress> {
        &self.progress
    }

    /// A receiver that observes the download's phase.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Phase> {
        self.phase.subscribe()
    }

    /// Marks the download as finished. Wakes everyone awaiting it.
    pub(crate) fn complete(&self, phase: Phase) {
        let _ = self.phase.send(phase);
    }

    /// A snapshot of this download for display, labelled with `key`.
    pub(crate) fn snapshot(&self, key: &str) -> DownloadProgress {
        DownloadProgress {
            key: key.to_owned(),
            source: self.source.clone(),
            started_at: self.started_at,
            done_bytes: self.progress.done(),
            total_bytes: self.progress.total(),
        }
    }
}

/// A snapshot of one ongoing download, for display.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Logical artifact key.
    pub key: String,
    /// Where it is being fetched from.
    pub source: String,
    /// When the download started.
    pub started_at: Timestamp,
    /// Bytes transferred so far.
    pub done_bytes: u64,
    /// Expected total bytes, if known.
    pub total_bytes: Option<u64>,
}

/// The result of claiming a download slot for an artifact.
pub(crate) enum Claim {
    /// No download was in flight. The caller must run it and complete the slot.
    Owner(Arc<Slot>),
    /// A download is already in flight. The caller only observes it.
    Joiner(Arc<Slot>),
}

/// Tracks the downloads currently in flight, keyed by artifact name.
#[derive(Clone, Default)]
pub(crate) struct Downloads {
    active: Arc<Mutex<HashMap<String, Arc<Slot>>>>,
}

impl Downloads {
    /// Claims the slot for `name`. The first caller becomes the owner and must
    /// run the download. Later callers join the existing one.
    pub(crate) fn claim(&self, name: &str, source: &str) -> Claim {
        let mut active = self.active.lock().expect("downloads poisoned");
        if let Some(slot) = active.get(name) {
            return Claim::Joiner(Arc::clone(slot));
        }
        let (phase, _) = watch::channel(Phase::Running);
        let slot = Arc::new(Slot {
            source: source.to_owned(),
            started_at: Timestamp::now(),
            progress: Arc::new(Progress::default()),
            phase,
        });
        active.insert(name.to_owned(), Arc::clone(&slot));
        Claim::Owner(slot)
    }

    /// Removes the slot for `name` from the active set.
    pub(crate) fn release(&self, name: &str) {
        self.active.lock().expect("downloads poisoned").remove(name);
    }

    /// Returns whether a download for `name` is currently in flight.
    pub(crate) fn is_active(&self, name: &str) -> bool {
        self.active
            .lock()
            .expect("downloads poisoned")
            .contains_key(name)
    }

    /// A snapshot of all downloads currently in flight.
    pub(crate) fn snapshot(&self) -> Vec<DownloadProgress> {
        let active = self.active.lock().expect("downloads poisoned");
        active
            .iter()
            .map(|(key, slot)| slot.snapshot(key))
            .collect()
    }
}

/// An [`AsyncWrite`] that forwards to `inner` and counts bytes into `progress`.
pub(crate) struct ProgressWriter<'a, W> {
    inner: W,
    progress: &'a Progress,
}

impl<'a, W> ProgressWriter<'a, W> {
    pub(crate) fn new(inner: W, progress: &'a Progress) -> Self {
        Self { inner, progress }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ProgressWriter<'_, W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = ready!(Pin::new(&mut this.inner).poll_write(cx, buf))?;
        this.progress.add(written as u64);
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_coalesces_until_released() {
        let downloads = Downloads::default();

        let Claim::Owner(owner) = downloads.claim("spectra", "src") else {
            panic!("first claim should own the slot");
        };
        let Claim::Joiner(joiner) = downloads.claim("spectra", "src") else {
            panic!("second claim should join the slot");
        };
        assert!(Arc::ptr_eq(&owner, &joiner));
        assert!(downloads.is_active("spectra"));

        owner.complete(Phase::Succeeded);
        downloads.release("spectra");
        assert!(!downloads.is_active("spectra"));

        // A fresh claim after release owns a new slot.
        assert!(matches!(downloads.claim("spectra", "src"), Claim::Owner(_)));
    }

    #[tokio::test]
    async fn joiner_observes_completion() {
        let downloads = Downloads::default();
        let Claim::Owner(owner) = downloads.claim("spectra", "src") else {
            panic!("expected owner");
        };
        let Claim::Joiner(joiner) = downloads.claim("spectra", "src") else {
            panic!("expected joiner");
        };

        let mut phase = joiner.subscribe();
        owner.complete(Phase::Succeeded);
        while *phase.borrow_and_update() == Phase::Running {
            phase.changed().await.unwrap();
        }
        assert_eq!(*phase.borrow(), Phase::Succeeded);
    }

    #[test]
    fn snapshot_reports_progress() {
        let downloads = Downloads::default();
        let Claim::Owner(owner) = downloads.claim("spectra", "ghcr.io/x") else {
            panic!("expected owner");
        };
        owner.progress().set_total(100);
        owner.progress().add(40);

        let snapshot = downloads.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].key, "spectra");
        assert_eq!(snapshot[0].done_bytes, 40);
        assert_eq!(snapshot[0].total_bytes, Some(100));
    }
}
