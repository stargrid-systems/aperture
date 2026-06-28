//! The artifacts store: fetches artifacts into the blob store, records them in
//! the storage catalog, tracks ongoing downloads, and keeps the two consistent.

use std::collections::HashSet;
use std::error::Error;
use std::future::{Future, IntoFuture};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use aperture_storage::{
    Artifact, ArtifactKey, Download, DownloadStatus, ListQuery, Page, Storage, VersionSort,
};
use jiff::Timestamp;
use oci_client::Reference;
use tokio::fs;

use crate::blob::BlobStore;
use crate::digest::Digest;
use crate::downloads::{Claim, DownloadProgress, Downloads, Phase, Progress, ProgressWriter, Slot};
use crate::error::{ArtifactError, Result};
use crate::fetch::{FetchMeta, Fetched, OciFetcher};
use crate::hash_writer::HashWriter;
use crate::media_type::MediaType;

/// What a [`Artifacts::sync`] run removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    /// Blobs on disk that no catalog entry referenced.
    pub removed_blobs: usize,
    /// Catalog entries whose blob was missing from disk.
    pub removed_entries: usize,
}

/// A present artifact located in the blob store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// Absolute path of the stored blob.
    pub path: PathBuf,
    /// Content digest of the stored blob.
    pub digest: Digest,
}

/// What to fetch. The source carries everything a fetcher needs.
#[derive(Debug, Clone)]
pub struct FetchRequest {
    /// Logical key to record the artifact under.
    pub key: String,
    /// Where and how to fetch it from.
    pub source: FetchSource,
}

impl FetchRequest {
    /// The source string recorded in the catalog and download history.
    fn source_str(&self) -> &str {
        match &self.source {
            FetchSource::Oci { reference, .. } => reference,
        }
    }
}

/// Where an artifact is fetched from. Extensible to more sources later.
#[derive(Debug, Clone)]
pub enum FetchSource {
    /// A layer of an OCI image.
    Oci {
        /// The image reference, for example `ghcr.io/org/image:tag`.
        reference: String,
        /// The media type of the layer to pull.
        media_type: MediaType,
    },
}

/// Coordinates fetching artifacts, recording them in the catalog, tracking
/// in-flight downloads, and keeping the catalog and blob store consistent.
///
/// Cheap to clone: all clones share one underlying manager.
#[derive(Clone)]
pub struct Artifacts {
    inner: Arc<Inner>,
}

struct Inner {
    storage: Storage,
    blobs: BlobStore,
    oci: OciFetcher,
    downloads: Downloads,
}

impl Artifacts {
    /// Creates a store backed by `storage`, keeping blobs under `store_root`.
    pub fn new(storage: Storage, store_root: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                storage,
                blobs: BlobStore::new(store_root),
                oci: OciFetcher::new(),
                downloads: Downloads::default(),
            }),
        }
    }

    /// Opens storage at `db_path` (applying migrations) and keeps blobs under
    /// `store_root`.
    pub async fn open(db_path: &str, store_root: PathBuf) -> Result<Self> {
        let storage = Storage::open(db_path).await?;
        Ok(Self::new(storage, store_root))
    }

    /// Read access to the storage catalog.
    pub fn storage(&self) -> &Storage {
        &self.inner.storage
    }

    /// A snapshot of the downloads currently in flight, for display.
    pub fn active_downloads(&self) -> Vec<DownloadProgress> {
        self.inner.downloads.snapshot()
    }

    /// Returns the blob of the newest stored version of `key`, if present on
    /// disk.
    pub async fn locate(&self, key: &str) -> Result<Option<Located>> {
        self.inner.locate(key).await
    }

    /// Returns the blob of the `(key, digest)` version, if present on disk.
    pub async fn locate_version(&self, key: &str, digest: &str) -> Result<Option<Located>> {
        self.inner.locate_version(key, digest).await
    }

    /// Lists stored artifact keys with their newest version and version count.
    /// `q` matches a substring of the key.
    pub async fn list_artifacts(
        &self,
        q: Option<&str>,
        query: &ListQuery,
    ) -> Result<Page<ArtifactKey>> {
        Ok(self.inner.storage.artifacts().list_keys(q, query).await?)
    }

    /// Returns one artifact key with its newest version and version count.
    pub async fn artifact(&self, key: &str) -> Result<Option<ArtifactKey>> {
        Ok(self.inner.storage.artifacts().get_key(key).await?)
    }

    /// Lists the stored versions of `key`, optionally filtered by exact
    /// `media_type` and `version`.
    pub async fn list_versions(
        &self,
        key: &str,
        sort: VersionSort,
        media_type: Option<&str>,
        version: Option<&str>,
        query: &ListQuery,
    ) -> Result<Page<Artifact>> {
        Ok(self
            .inner
            .storage
            .artifacts()
            .list_versions(key, sort, media_type, version, query)
            .await?)
    }

    /// Returns the `(key, digest)` version, if stored.
    pub async fn version(&self, key: &str, digest: &str) -> Result<Option<Artifact>> {
        Ok(self.inner.storage.artifacts().get_version(key, digest).await?)
    }

    /// Lists download attempts, optionally filtered by status and key.
    pub async fn list_downloads(
        &self,
        status: Option<DownloadStatus>,
        key: Option<&str>,
        query: &ListQuery,
    ) -> Result<Page<Download>> {
        Ok(self
            .inner
            .storage
            .artifacts()
            .list_downloads(status, key, query)
            .await?)
    }

    /// Removes the `(key, digest)` version, and its blob if no other version
    /// references it. Returns whether the version existed.
    pub async fn evict_version(&self, key: &str, digest: &str) -> Result<bool> {
        self.inner.evict_version(key, digest).await
    }

    /// Returns a handle to the artifact in `request`. If it is already present
    /// the handle is immediately ready. Otherwise a download starts in the
    /// background, or joins one already in flight, and the handle tracks it.
    pub async fn fetch(&self, request: FetchRequest) -> Result<DownloadHandle> {
        if let Some(located) = self.inner.locate(&request.key).await? {
            return Ok(DownloadHandle::ready(located));
        }

        let slot = match self.inner.downloads.claim(&request.key, request.source_str()) {
            Claim::Joiner(slot) => {
                return Ok(DownloadHandle::tracking(
                    Arc::clone(&self.inner),
                    request.key,
                    slot,
                ));
            }
            Claim::Owner(slot) => slot,
        };

        let started = Timestamp::now();
        let id = match self
            .inner
            .storage
            .artifacts()
            .start_download(&request.key, request.source_str(), started)
            .await
        {
            Ok(id) => id,
            Err(err) => {
                // Never started, so wake any joiner and free the slot.
                slot.complete(Phase::Failed);
                self.inner.downloads.release(&request.key);
                return Err(err.into());
            }
        };

        let inner = Arc::clone(&self.inner);
        let task_slot = Arc::clone(&slot);
        let key = request.key.clone();
        let run = request;
        tokio::spawn(async move {
            inner.run_download(run, id, task_slot).await;
        });

        Ok(DownloadHandle::tracking(Arc::clone(&self.inner), key, slot))
    }

    /// Fetches the artifact in `request` and waits for it to be ready.
    pub async fn ensure(&self, request: FetchRequest) -> Result<Located> {
        self.fetch(request).await?.await
    }

    /// Reconciles the catalog with the blob store. Marks interrupted downloads,
    /// removes catalog entries whose blob is missing, removes blobs that no
    /// entry references, and clears leftover temporary files.
    pub async fn sync(&self) -> Result<SyncReport> {
        self.inner.sync().await
    }
}

impl Inner {
    async fn locate(&self, key: &str) -> Result<Option<Located>> {
        match self.storage.artifacts().latest(key).await? {
            Some(artifact) => self.locate_digest(&artifact.digest).await,
            None => Ok(None),
        }
    }

    async fn locate_version(&self, key: &str, digest: &str) -> Result<Option<Located>> {
        match self.storage.artifacts().get_version(key, digest).await? {
            Some(artifact) => self.locate_digest(&artifact.digest).await,
            None => Ok(None),
        }
    }

    async fn locate_digest(&self, raw: &str) -> Result<Option<Located>> {
        let digest: Digest = raw.parse()?;
        if !self.blobs.contains(&digest).await {
            return Ok(None);
        }
        Ok(Some(Located {
            path: self.blobs.path(&digest),
            digest,
        }))
    }

    /// Removes a stored version, and its blob when no other version still
    /// references that digest. Returns whether the version existed.
    async fn evict_version(&self, key: &str, digest: &str) -> Result<bool> {
        let repository = self.storage.artifacts();
        let Some(artifact) = repository.get_version(key, digest).await? else {
            return Ok(false);
        };
        repository.delete_version(key, digest).await?;

        let still_referenced = repository
            .all_versions()
            .await?
            .iter()
            .any(|version| version.digest == artifact.digest);
        if !still_referenced
            && let Ok(parsed) = artifact.digest.parse::<Digest>()
        {
            self.blobs.remove(&parsed).await?;
        }
        Ok(true)
    }

    /// Runs the download for `request`. On success it records the new version;
    /// either way it records the outcome in the download history, then completes
    /// the slot and releases it. Runs on its own task, so failures are logged
    /// rather than returned.
    async fn run_download(self: Arc<Self>, request: FetchRequest, id: i64, slot: Arc<Slot>) {
        let repository = self.storage.artifacts();
        let result = self.execute(&request, slot.progress()).await;
        let finished = Timestamp::now();

        let phase = match &result {
            Ok(fetched) => {
                let artifact = version_artifact(&request, fetched, finished);
                if let Err(err) = repository.record_version(&artifact).await {
                    tracing::error!(key = %request.key, error = &err as &dyn Error, "failed to record artifact version");
                }
                if let Err(err) = repository
                    .finish_download(
                        id,
                        DownloadStatus::Succeeded,
                        finished,
                        Some(&fetched.digest.to_string()),
                        Some(fetched.size as i64),
                        None,
                    )
                    .await
                {
                    tracing::error!(key = %request.key, error = &err as &dyn Error, "failed to finish download record");
                }
                Phase::Succeeded
            }
            Err(err) => {
                if let Err(write_err) = repository
                    .finish_download(
                        id,
                        DownloadStatus::Failed,
                        finished,
                        None,
                        None,
                        Some(&err.to_string()),
                    )
                    .await
                {
                    tracing::error!(key = %request.key, error = &write_err as &dyn Error, "failed to finish download record");
                }
                Phase::Failed
            }
        };

        slot.complete(phase);
        self.downloads.release(&request.key);
    }

    /// Dispatches a fetch to the right fetcher for its source.
    async fn execute(&self, request: &FetchRequest, progress: &Progress) -> Result<Fetched> {
        match &request.source {
            FetchSource::Oci {
                reference,
                media_type,
            } => {
                let reference: Reference = reference.parse().map_err(|err| {
                    ArtifactError::Fetch(anyhow::format_err!("invalid reference {reference:?}: {err}"))
                })?;
                self.fetch_oci(&reference, media_type, progress).await
            }
        }
    }

    /// Stages an OCI fetch into the blob store: streams the layer into a
    /// temporary file, verifies the bytes against the advertised digest, then
    /// places the blob under its digest. The fetcher only ever sees the sink, so
    /// it cannot reach the store on its own.
    async fn fetch_oci(
        &self,
        reference: &Reference,
        media_type: &MediaType,
        progress: &Progress,
    ) -> Result<Fetched> {
        let mut temp = self.blobs.temp_file().await?;

        let staged: Result<(FetchMeta, Digest, u64)> = async {
            let mut writer = HashWriter::new(ProgressWriter::new(&mut *temp, progress));
            let meta = self
                .oci
                .fetch(reference, media_type, &mut writer, progress)
                .await?;
            let (digest, size) = writer.finalize().await?;
            Ok((meta, digest, size))
        }
        .await;

        let (meta, digest, size) = match staged {
            Ok(staged) => staged,
            Err(err) => {
                let _ = fs::remove_file(temp.path()).await;
                return Err(err);
            }
        };

        if digest != meta.expected_digest {
            let _ = fs::remove_file(temp.path()).await;
            return Err(ArtifactError::DigestMismatch {
                expected: meta.expected_digest.to_string(),
                actual: digest.to_string(),
            });
        }

        self.blobs.place(temp.path(), &digest).await?;
        Ok(Fetched {
            digest,
            media_type: meta.media_type,
            version: reference.tag().map(str::to_owned),
            size,
        })
    }

    async fn sync(&self) -> Result<SyncReport> {
        self.blobs.clear_temp().await?;

        let repository = self.storage.artifacts();
        let mut report = SyncReport::default();
        let mut tracked: HashSet<Digest> = HashSet::new();

        // A download still marked running has no owner left after a restart.
        let now = Timestamp::now();
        for download in repository.list_running().await? {
            if self.downloads.is_active(&download.artifact) {
                continue;
            }
            repository
                .finish_download(
                    download.id,
                    DownloadStatus::Interrupted,
                    now,
                    None,
                    None,
                    Some("interrupted"),
                )
                .await?;
        }

        for artifact in repository.all_versions().await? {
            let Ok(digest) = artifact.digest.parse::<Digest>() else {
                // An unreadable digest can never match a blob, so drop the row.
                repository
                    .delete_version(&artifact.key, &artifact.digest)
                    .await?;
                report.removed_entries += 1;
                continue;
            };
            if self.blobs.contains(&digest).await {
                tracked.insert(digest);
            } else {
                repository
                    .delete_version(&artifact.key, &artifact.digest)
                    .await?;
                report.removed_entries += 1;
            }
        }

        for digest in self.blobs.list().await? {
            if !tracked.contains(&digest) {
                self.blobs.remove(&digest).await?;
                report.removed_blobs += 1;
            }
        }

        Ok(report)
    }
}

/// A handle to a requested artifact. Cheap to clone, so several callers can
/// observe the same download. Await it for the stored blob, or read live
/// [`DownloadHandle::progress`] while it runs.
#[derive(Clone)]
pub struct DownloadHandle {
    state: HandleState,
}

#[derive(Clone)]
enum HandleState {
    Ready(Located),
    Tracking {
        inner: Arc<Inner>,
        key: String,
        slot: Arc<Slot>,
    },
}

impl DownloadHandle {
    fn ready(located: Located) -> Self {
        Self {
            state: HandleState::Ready(located),
        }
    }

    fn tracking(inner: Arc<Inner>, key: String, slot: Arc<Slot>) -> Self {
        Self {
            state: HandleState::Tracking { inner, key, slot },
        }
    }

    /// The artifact's logical key.
    pub fn key(&self) -> Option<&str> {
        match &self.state {
            HandleState::Ready(_) => None,
            HandleState::Tracking { key, .. } => Some(key),
        }
    }

    /// Live progress of the download, or `None` if it is already present.
    pub fn progress(&self) -> Option<DownloadProgress> {
        match &self.state {
            HandleState::Ready(_) => None,
            HandleState::Tracking { key, slot, .. } => Some(slot.snapshot(key)),
        }
    }

    /// Waits for the artifact to be ready and returns where its blob landed.
    pub async fn wait(self) -> Result<Located> {
        match self.state {
            HandleState::Ready(located) => Ok(located),
            HandleState::Tracking { inner, key, slot } => {
                let mut phase = slot.subscribe();
                while *phase.borrow_and_update() == Phase::Running {
                    if phase.changed().await.is_err() {
                        break;
                    }
                }
                let final_phase = *phase.borrow();
                match final_phase {
                    Phase::Succeeded => inner.locate(&key).await?.ok_or_else(|| {
                        ArtifactError::Fetch(anyhow::format_err!("{key} missing after download"))
                    }),
                    _ => Err(ArtifactError::Fetch(anyhow::format_err!(
                        "download of {key} failed"
                    ))),
                }
            }
        }
    }
}

impl IntoFuture for DownloadHandle {
    type Output = Result<Located>;
    type IntoFuture = Pin<Box<dyn Future<Output = Result<Located>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.wait())
    }
}

fn version_artifact(request: &FetchRequest, fetched: &Fetched, finished: Timestamp) -> Artifact {
    Artifact {
        id: 0,
        key: request.key.clone(),
        source: request.source_str().to_owned(),
        digest: fetched.digest.to_string(),
        media_type: Some(fetched.media_type.to_string()),
        version: fetched.version.clone(),
        size_bytes: fetched.size as i64,
        downloaded_at: finished,
        verified_at: Some(finished),
    }
}
