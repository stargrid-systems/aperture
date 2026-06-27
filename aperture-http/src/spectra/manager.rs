//! The Spectra frontend's lifecycle: where to get it, the open image, and
//! whether a download is in flight.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use aperture_artifacts::{ArtifactKind, Artifacts, FetchRequest, FetchSource};
use tokio::time;

use super::config::SpectraConfig;
use super::image::{SpectraImage, open_image};

/// Owns the Spectra frontend and fetches it on demand.
#[derive(Clone)]
pub struct Spectra {
    artifacts: Arc<Artifacts>,
    config: SpectraConfig,
    current: Arc<RwLock<Option<Arc<SpectraImage>>>>,
    preparing: Arc<AtomicBool>,
}

impl Spectra {
    /// Creates a frontend backed by `artifacts`, pulling from `config`.
    pub fn new(artifacts: Arc<Artifacts>, config: SpectraConfig) -> Self {
        Self {
            artifacts,
            config,
            current: Arc::new(RwLock::new(None)),
            preparing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The artifact manager behind this frontend.
    pub fn artifacts(&self) -> &Arc<Artifacts> {
        &self.artifacts
    }

    /// Opens the frontend if its blob is already cached, without downloading.
    pub async fn activate_if_present(&self) -> anyhow::Result<()> {
        if let Some(located) = self.artifacts.locate(&self.config.name).await? {
            let image = open_image(located.path, located.digest.to_string()).await?;
            self.set(Arc::new(image));
        }
        Ok(())
    }

    /// Downloads the frontend if needed, then opens it. Awaits completion.
    pub async fn prefetch(&self) -> anyhow::Result<()> {
        self.prepare().await
    }

    /// Starts a background download of the frontend if it is not already
    /// present. Returns immediately.
    pub fn start_prefetch(&self) {
        self.ensure_started();
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
        // TODO: switch to a proper task management system!
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(error) = this.prepare().await {
                tracing::error!(%error, "failed to prepare spectra frontend");
                time::sleep(Duration::from_secs(30)).await;
            }
            this.preparing.store(false, Ordering::SeqCst);
        });
    }

    async fn prepare(&self) -> anyhow::Result<()> {
        let located = self
            .artifacts
            .ensure(FetchRequest {
                name: self.config.name.clone(),
                kind: ArtifactKind::Oci,
                source: FetchSource::Oci {
                    reference: self.config.source.clone(),
                    media_type: self.config.media_type.clone(),
                },
            })
            .await?;
        let image = open_image(located.path, located.digest.to_string()).await?;
        self.set(Arc::new(image));
        Ok(())
    }
}
