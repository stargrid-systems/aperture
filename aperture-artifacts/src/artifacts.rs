//! The artifacts store: fetches artifacts into the blob store, records them in
//! the storage catalog, and keeps the two consistent.
//!
//! Downloading is exposed as a single [`Artifacts::download`] call. The task
//! system (see [`crate::DownloadDefinition`]) drives it, tracks progress, and
//! records each invocation. This layer just does the work.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use aperture_storage::{Artifact, ArtifactKey, ListQuery, Page, Storage, VersionSort};
use aperture_tasks::ProgressHandle;
use jiff::Timestamp;
use oci_client::Reference;
use tokio::fs;

use crate::blob::BlobStore;
use crate::digest::Digest;
use crate::error::{ArtifactError, Result};
use crate::fetch::{FetchMeta, Fetched, OciFetcher};
use crate::hash_writer::HashWriter;
use crate::media_type::MediaType;
use crate::progress::ProgressWriter;

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
    /// The source string recorded in the catalog.
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

/// Coordinates fetching artifacts, recording them in the catalog, and keeping
/// the catalog and blob store consistent.
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
}

impl Artifacts {
    /// Creates a store backed by `storage`, keeping blobs under `store_root`.
    pub fn new(storage: Storage, store_root: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                storage,
                blobs: BlobStore::new(store_root),
                oci: OciFetcher::new(),
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

    /// Removes the `(key, digest)` version, and its blob if no other version
    /// references it. Returns whether the version existed.
    pub async fn evict_version(&self, key: &str, digest: &str) -> Result<bool> {
        self.inner.evict_version(key, digest).await
    }

    /// Ensures the artifact in `request` is present, downloading it if needed,
    /// and returns the stored version. If the newest version is already on disk
    /// it is returned without fetching. Transferred bytes are reported into
    /// `progress`.
    pub async fn download(
        &self,
        request: FetchRequest,
        progress: ProgressHandle,
    ) -> Result<Artifact> {
        self.inner.download(&request, &progress).await
    }

    /// Reconciles the catalog with the blob store. Removes catalog entries whose
    /// blob is missing, removes blobs that no entry references, and clears
    /// leftover temporary files.
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

    /// Returns the newest present version of `key`, or fetches it. The cached
    /// version is reused only when it came from the same source; a repointed
    /// source is always fetched. On a fetch it stages the bytes, verifies the
    /// digest, and records the new version.
    async fn download(&self, request: &FetchRequest, progress: &ProgressHandle) -> Result<Artifact> {
        let repository = self.storage.artifacts();
        if let Some(latest) = repository.latest(&request.key).await?
            && latest.source == request.source_str()
            && let Ok(digest) = latest.digest.parse::<Digest>()
            && self.blobs.contains(&digest).await
        {
            return Ok(latest);
        }

        let fetched = self.execute(request, progress).await?;
        let finished = Timestamp::now();
        let artifact = version_artifact(request, &fetched, finished);
        repository.record_version(&artifact).await?;
        Ok(artifact)
    }

    /// Dispatches a fetch to the right fetcher for its source.
    async fn execute(&self, request: &FetchRequest, progress: &ProgressHandle) -> Result<Fetched> {
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
        progress: &ProgressHandle,
    ) -> Result<Fetched> {
        let mut temp = self.blobs.temp_file().await?;

        let staged: Result<(FetchMeta, Digest, u64)> = async {
            let mut writer = HashWriter::new(ProgressWriter::new(&mut *temp, progress.clone()));
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
