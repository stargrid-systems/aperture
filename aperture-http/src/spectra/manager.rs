//! The Spectra frontend's lifecycle: where to get it, the open image, and
//! whether a download is in flight.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use aperture_artifacts::{Artifacts, DownloadDefinition, DownloadInput, DownloadSource};
use aperture_runtime::{Stop, Worker};
use aperture_tasks::Tasks;
use tokio::task::JoinHandle;
use tokio::time;
use tokio_util::sync::CancellationToken;

use super::config::SpectraConfig;
use super::image::{SpectraImage, open_image};

/// Backoff after a failed prepare attempt. The spectra loader is best-effort:
/// if the registry is unreachable we wait and let the next HTTP request retry.
const PREPARE_BACKOFF: Duration = Duration::from_secs(30);

/// Owns the Spectra frontend and fetches it on demand.
///
/// Cheap to clone: all state is shared through `Arc`.
#[derive(Clone)]
pub struct Spectra {
    inner: Arc<SpectraInner>,
}

struct SpectraInner {
    artifacts: Artifacts,
    tasks: Tasks,
    config: SpectraConfig,
    current: RwLock<Option<Arc<SpectraImage>>>,
    preparing: Arc<AtomicBool>,
    /// Cancels the in-flight prepare task, if any. Cloned into the task so
    /// `shutdown` can interrupt the backoff sleep.
    cancel: CancellationToken,
    /// The join handle of the in-flight prepare task, if any. Stored so
    /// `shutdown` can abort it instead of leaving it detached.
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Spectra {
    /// Creates a frontend backed by `artifacts`, fetched via `tasks`, pulling
    /// from `config`.
    pub fn new(artifacts: Artifacts, tasks: Tasks, config: SpectraConfig) -> Self {
        Self {
            inner: Arc::new(SpectraInner {
                artifacts,
                tasks,
                config,
                current: RwLock::new(None),
                preparing: Arc::new(AtomicBool::new(false)),
                cancel: CancellationToken::new(),
                task: Mutex::new(None),
            }),
        }
    }

    /// The artifact manager behind this frontend.
    pub fn artifacts(&self) -> &Artifacts {
        &self.inner.artifacts
    }

    /// Opens the frontend if its blob is already cached, without downloading.
    ///
    /// Returns whether a cached blob was found and opened.
    pub async fn activate_if_present(&self) -> anyhow::Result<bool> {
        if let Some(located) = self.inner.artifacts.locate(&self.inner.config.key).await? {
            let image = open_image(located.path, located.digest).await?;
            self.set(Arc::new(image));
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn current(&self) -> Option<Arc<SpectraImage>> {
        self.inner
            .current
            .read()
            .expect("spectra slot poisoned")
            .clone()
    }

    fn set(&self, image: Arc<SpectraImage>) {
        *self.inner.current.write().expect("spectra slot poisoned") = Some(image);
    }

    /// Starts a background download and open, unless one is already running or
    /// the frontend is already present.
    pub(super) fn ensure_started(&self) {
        if self.current().is_some() {
            return;
        }
        if self.inner.preparing.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        let cancel = self.inner.cancel.clone();
        // The guard resets `preparing` on drop, including on panic. Without
        // this, a panic in `prepare` would leave `preparing = true` forever
        // and the frontend would never load again.
        let preparing = Arc::clone(&self.inner.preparing);
        let handle = tokio::spawn(async move {
            let _guard = PreparingGuard(preparing);
            let outcome = tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                result = this.prepare() => result,
            };
            if let Err(err) = outcome {
                tracing::error!(error = &*err, "failed to prepare spectra frontend");
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => {},
                    _ = time::sleep(PREPARE_BACKOFF) => {}
                }
            }
        });
        *self.inner.task.lock().expect("spectra task slot poisoned") = Some(handle);
    }

    async fn prepare(&self) -> anyhow::Result<()> {
        let input = DownloadInput {
            key: self.inner.config.key.clone(),
            source: DownloadSource::Oci {
                reference: self.inner.config.source.clone(),
                media_type: self.inner.config.media_type.clone(),
            },
        };
        self.inner
            .tasks
            .spawn::<DownloadDefinition>(input)
            .await?
            .wait()
            .await?;
        if !self.activate_if_present().await? {
            anyhow::bail!(
                "spectra artifact {:?} missing after download",
                self.inner.config.key
            );
        }
        Ok(())
    }

    /// Aborts any in-flight prepare task. The supervisor calls this on
    /// shutdown via [`SpectraWorker`].
    fn shutdown(&self) {
        self.inner.cancel.cancel();
        if let Some(handle) = self
            .inner
            .task
            .lock()
            .expect("spectra task slot poisoned")
            .take()
        {
            handle.abort();
        }
    }
}

/// Resets the `preparing` flag when dropped, even on panic.
struct PreparingGuard(Arc<AtomicBool>);

impl Drop for PreparingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Supervisor-facing worker that drives [`Spectra`] shutdown.
///
/// Construct with [`SpectraWorker::new`] using a clone of the [`Spectra`] that
/// is also shared with HTTP handlers. The worker itself does not run the
/// download. Downloads are triggered lazily by HTTP requests. The worker only
/// makes sure any in-flight prepare task is cancelled and joined before the
/// process exits.
pub struct SpectraWorker(Spectra);

impl SpectraWorker {
    /// Wraps a clone of `spectra` for supervisor tracking.
    pub fn new(spectra: Spectra) -> Self {
        Self(spectra)
    }
}

impl Worker for SpectraWorker {
    async fn run(self, stop: Stop) {
        stop.cancelled().await;
        self.0.shutdown();
    }
}
