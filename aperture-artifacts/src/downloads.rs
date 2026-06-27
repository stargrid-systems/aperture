//! In-memory tracking of ongoing downloads.
//!
//! The tracker does two jobs at once. It coalesces concurrent requests for the
//! same artifact onto a single download (single-flight), and it exposes live
//! progress so the frontend can show what is being fetched right now. Completed
//! attempts live in the persistent download history, not here.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};

use jiff::Timestamp;
use tokio::io::AsyncWrite;
use tokio::sync::watch;

use crate::error::{ArtifactError, Result};

/// Terminal or running state of a single download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
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

struct Slot {
    source: String,
    started_at: Timestamp,
    progress: Arc<Progress>,
    phase: watch::Sender<Phase>,
}

/// A snapshot of one ongoing download, for display.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Logical artifact name.
    pub name: String,
    /// Where it is being fetched from.
    pub source: String,
    /// When the download started.
    pub started_at: Timestamp,
    /// Bytes transferred so far.
    pub done_bytes: u64,
    /// Expected total bytes, if known.
    pub total_bytes: Option<u64>,
}

/// Tracks the downloads currently in flight.
#[derive(Clone, Default)]
pub(crate) struct Downloads {
    active: Arc<Mutex<HashMap<String, Arc<Slot>>>>,
}

impl Downloads {
    /// Runs `op` as the single download for `name`, or waits for the download
    /// already in flight for `name` and reports its outcome.
    ///
    /// Only the first caller runs `op`. Later callers wait for it to finish.
    /// `op` receives a [`Progress`] handle to report bytes against.
    pub(crate) async fn run<F, Fut>(&self, name: &str, source: &str, op: F) -> Result<()>
    where
        F: FnOnce(Arc<Progress>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let role = {
            let mut active = self.active.lock().unwrap();
            match active.get(name) {
                Some(slot) => Role::Joiner(slot.phase.subscribe()),
                None => {
                    let (phase, _) = watch::channel(Phase::Running);
                    let slot = Arc::new(Slot {
                        source: source.to_owned(),
                        started_at: Timestamp::now(),
                        progress: Arc::new(Progress::default()),
                        phase,
                    });
                    active.insert(name.to_owned(), Arc::clone(&slot));
                    Role::Owner(slot)
                }
            }
        };

        match role {
            Role::Owner(slot) => {
                let result = op(Arc::clone(&slot.progress)).await;
                let phase = if result.is_ok() {
                    Phase::Succeeded
                } else {
                    Phase::Failed
                };
                let _ = slot.phase.send(phase);
                self.active.lock().unwrap().remove(name);
                result
            }
            Role::Joiner(mut phase) => {
                while *phase.borrow_and_update() == Phase::Running {
                    if phase.changed().await.is_err() {
                        break;
                    }
                }
                match *phase.borrow() {
                    Phase::Succeeded => Ok(()),
                    _ => Err(ArtifactError::Fetch(anyhow::format_err!(
                        "a concurrent download of {name} failed"
                    ))),
                }
            }
        }
    }

    /// A snapshot of all downloads currently in flight.
    pub(crate) fn snapshot(&self) -> Vec<DownloadProgress> {
        let active = self.active.lock().unwrap();
        active
            .iter()
            .map(|(name, slot)| DownloadProgress {
                name: name.clone(),
                source: slot.source.clone(),
                started_at: slot.started_at,
                done_bytes: slot.progress.done(),
                total_bytes: slot.progress.total(),
            })
            .collect()
    }
}

enum Role {
    Owner(Arc<Slot>),
    Joiner(watch::Receiver<Phase>),
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
    use std::sync::atomic::AtomicU32;
    use std::time::Duration;

    use tokio::sync::Notify;
    use tokio::time::sleep;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn coalesces_concurrent_runs() {
        let downloads = Downloads::default();
        let runs = Arc::new(AtomicU32::new(0));
        let gate = Arc::new(Notify::new());

        let make = || {
            let runs = Arc::clone(&runs);
            let gate = Arc::clone(&gate);
            move |_progress: Arc<Progress>| async move {
                runs.fetch_add(1, Ordering::Relaxed);
                gate.notified().await;
                Ok(())
            }
        };

        let owner = tokio::spawn({
            let downloads = downloads.clone();
            let op = make();
            async move { downloads.run("spectra", "src", op).await }
        });
        let joiner = tokio::spawn({
            let downloads = downloads.clone();
            let op = make();
            async move { downloads.run("spectra", "src", op).await }
        });

        sleep(Duration::from_millis(50)).await;
        gate.notify_one();

        owner.await.unwrap().unwrap();
        joiner.await.unwrap().unwrap();
        assert_eq!(runs.load(Ordering::Relaxed), 1);
        assert!(downloads.snapshot().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reports_live_progress() {
        let downloads = Downloads::default();
        let gate = Arc::new(Notify::new());

        let running = tokio::spawn({
            let downloads = downloads.clone();
            let gate = Arc::clone(&gate);
            async move {
                downloads
                    .run("spectra", "ghcr.io/x", move |progress| async move {
                        progress.set_total(100);
                        progress.add(40);
                        gate.notified().await;
                        Ok(())
                    })
                    .await
            }
        });

        sleep(Duration::from_millis(50)).await;
        let snapshot = downloads.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].name, "spectra");
        assert_eq!(snapshot[0].done_bytes, 40);
        assert_eq!(snapshot[0].total_bytes, Some(100));

        gate.notify_one();
        running.await.unwrap().unwrap();
        assert!(downloads.snapshot().is_empty());
    }
}
