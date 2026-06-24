//! The artifacts store: fetches artifacts into the blob store, records them in
//! the storage catalog, tracks ongoing downloads, and keeps the two consistent.

use std::collections::HashSet;
use std::path::PathBuf;

use aperture_storage::{
    Artifact, ArtifactKind, ArtifactStatus, DownloadResult, NewDownload, Storage,
};
use jiff::Timestamp;
use oci_client::Reference;

use crate::blob::{BlobStore, Digest};
use crate::downloads::{DownloadProgress, Downloads, Progress};
use crate::error::{ArtifactError, Result};
use crate::fetch::OciFetcher;
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

/// Coordinates fetching artifacts, recording them in the catalog, tracking
/// in-flight downloads, and keeping the catalog and blob store consistent.
pub struct Artifacts {
    storage: Storage,
    blobs: BlobStore,
    oci: OciFetcher,
    downloads: Downloads,
}

impl Artifacts {
    /// Creates a store backed by `storage`, keeping blobs under `store_root`.
    pub fn new(storage: Storage, store_root: PathBuf) -> Self {
        Self {
            storage,
            blobs: BlobStore::new(store_root),
            oci: OciFetcher::new(),
            downloads: Downloads::default(),
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
        &self.storage
    }

    /// A snapshot of the downloads currently in flight, for display.
    pub fn active_downloads(&self) -> Vec<DownloadProgress> {
        self.downloads.snapshot()
    }

    /// Returns the stored blob for `name`, if it is present on disk.
    pub async fn locate(&self, name: &str) -> Result<Option<Located>> {
        let Some(artifact) = self.storage.artifacts().get(name).await? else {
            return Ok(None);
        };
        let Some(raw) = artifact.digest.as_deref() else {
            return Ok(None);
        };
        let digest: Digest = raw.parse()?;
        if !self.blobs.contains(&digest).await {
            return Ok(None);
        }
        Ok(Some(Located {
            path: self.blobs.path(&digest),
            digest,
        }))
    }

    /// Ensures the OCI artifact `name` is present, fetching it from `source`
    /// only if it is missing. Concurrent calls for the same `name` coalesce
    /// onto one download. Returns where the blob landed.
    pub async fn ensure_oci(
        &self,
        name: &str,
        kind: ArtifactKind,
        source: &str,
        media_type: &MediaType,
    ) -> Result<Located> {
        if let Some(located) = self.locate(name).await? {
            return Ok(located);
        }

        self.downloads
            .run(name, source, |progress| async move {
                self.download_oci(name, kind, source, media_type, &progress)
                    .await
            })
            .await?;

        self.locate(name).await?.ok_or_else(|| {
            ArtifactError::Fetch(anyhow::format_err!("{name} missing after download"))
        })
    }

    /// Fetches the `media_type` layer of the OCI image at `source` into the
    /// store and records it in the catalog under `name`. Every attempt, success
    /// or failure, is logged to the download history.
    async fn download_oci(
        &self,
        name: &str,
        kind: ArtifactKind,
        source: &str,
        media_type: &MediaType,
        progress: &Progress,
    ) -> Result<()> {
        let reference: Reference = source.parse().map_err(|err| {
            ArtifactError::Fetch(anyhow::format_err!("invalid reference {source:?}: {err}"))
        })?;
        let started = Timestamp::now();
        let repository = self.storage.artifacts();

        match self
            .oci
            .fetch(&reference, media_type, &self.blobs, progress)
            .await
        {
            Ok(fetched) => {
                let finished = Timestamp::now();
                let artifact = Artifact {
                    name: name.to_owned(),
                    kind,
                    source: source.to_owned(),
                    digest: Some(fetched.digest.to_string()),
                    media_type: Some(fetched.media_type.to_string()),
                    version: reference.tag().map(str::to_owned),
                    size_bytes: Some(fetched.size as i64),
                    blob_path: Some(
                        self.blobs
                            .relative_path(&fetched.digest)
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    status: ArtifactStatus::Present,
                    downloaded_at: Some(finished),
                    verified_at: Some(finished),
                };
                repository.upsert(&artifact).await?;
                repository
                    .record_download(&NewDownload {
                        artifact: name.to_owned(),
                        started_at: started,
                        finished_at: Some(finished),
                        result: DownloadResult::Success,
                        digest: artifact.digest.clone(),
                        size_bytes: artifact.size_bytes,
                        source: source.to_owned(),
                        error: None,
                    })
                    .await?;
                Ok(())
            }
            Err(err) => {
                let finished = Timestamp::now();
                repository
                    .record_download(&NewDownload {
                        artifact: name.to_owned(),
                        started_at: started,
                        finished_at: Some(finished),
                        result: DownloadResult::Failure,
                        digest: None,
                        size_bytes: None,
                        source: source.to_owned(),
                        error: Some(err.to_string()),
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Reconciles the catalog with the blob store. Removes catalog entries
    /// whose blob is missing from disk, removes blobs on disk that no entry
    /// references, and clears leftover temporary files.
    pub async fn sync(&self) -> Result<SyncReport> {
        self.blobs.clear_temp().await?;

        let repository = self.storage.artifacts();
        let mut report = SyncReport::default();
        let mut tracked: HashSet<Digest> = HashSet::new();

        for artifact in repository.list().await? {
            let Some(raw) = artifact.digest.as_deref() else {
                continue;
            };
            let Ok(digest) = raw.parse::<Digest>() else {
                continue;
            };
            if self.blobs.contains(&digest).await {
                tracked.insert(digest);
            } else {
                repository.delete(&artifact.name).await?;
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
