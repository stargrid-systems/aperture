//! The Spectra frontend's lifecycle: where to get it, the open image, and
//! whether a download is in flight.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use aperture_artifacts::{Artifacts, DownloadDefinition, DownloadInput, DownloadSource};
use aperture_storage::ActorId;
use aperture_tasks::Tasks;
use tokio::time;

use super::config::SpectraConfig;
use super::image::{SpectraImage, open_image};

/// Owns the Spectra frontend and fetches it on demand.
#[derive(Clone)]
pub struct Spectra {
    artifacts: Arc<Artifacts>,
    tasks: Tasks,
    config: SpectraConfig,
    system_actor: ActorId,
    current: Arc<RwLock<Option<Arc<SpectraImage>>>>,
    preparing: Arc<AtomicBool>,
}

impl Spectra {
    /// Creates a frontend backed by `artifacts`, fetched via `tasks`, pulling
    /// from `config`. `system_actor` is the actor used for internally spawned
    /// download tasks.
    pub fn new(
        artifacts: Arc<Artifacts>,
        tasks: Tasks,
        config: SpectraConfig,
        system_actor: ActorId,
    ) -> Self {
        Self {
            artifacts,
            tasks,
            config,
            system_actor,
            current: Arc::new(RwLock::new(None)),
            preparing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The artifact manager behind this frontend.
    pub fn artifacts(&self) -> &Arc<Artifacts> {
        &self.artifacts
    }

    /// Opens the frontend if its blob is already cached, without downloading.
    /// Returns whether a cached blob was found and opened.
    pub async fn activate_if_present(&self) -> anyhow::Result<bool> {
        if let Some(located) = self.artifacts.locate(&self.config.name).await? {
            let image = open_image(located.path, located.digest.to_string()).await?;
            self.set(Arc::new(image));
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn current(&self) -> Option<Arc<SpectraImage>> {
        self.current.read().expect("spectra slot poisoned").clone()
    }

    fn set(&self, image: Arc<SpectraImage>) {
        *self.current.write().expect("spectra slot poisoned") = Some(image);
    }

    /// Starts a background download and open, unless one is already running or
    /// the frontend is already present.
    pub(super) fn ensure_started(&self) {
        if self.current().is_some() {
            return;
        }
        if self.preparing.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(err) = this.prepare().await {
                tracing::error!(error = &*err, "failed to prepare spectra frontend");
                time::sleep(Duration::from_secs(30)).await;
            }
            this.preparing.store(false, Ordering::SeqCst);
        });
    }

    async fn prepare(&self) -> anyhow::Result<()> {
        let input = DownloadInput {
            key: self.config.name.clone(),
            source: DownloadSource::Oci {
                reference: self.config.source.clone(),
                media_type: self.config.media_type.as_str().to_owned(),
            },
        };
        self.tasks
            .spawn::<DownloadDefinition>(input, self.system_actor)
            .await?
            .wait()
            .await?;
        if !self.activate_if_present().await? {
            anyhow::bail!(
                "spectra artifact {:?} missing after download",
                self.config.name
            );
        }
        Ok(())
    }
}
