//! The artifacts store: fetches artifacts into the blob store, records them in
//! the storage catalog, and keeps the two consistent.

use std::collections::HashSet;
use std::path::PathBuf;

use aperture_storage::{
    Artifact, ArtifactKind, ArtifactStatus, DownloadResult, NewDownload, Storage,
};
use jiff::Timestamp;
use oci_client::Reference;

use crate::blob::{BlobStore, Digest};
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

/// Coordinates fetching artifacts, recording them in the catalog, and keeping
/// the catalog and blob store consistent.
pub struct Artifacts {
    storage: Storage,
    blobs: BlobStore,
    oci: OciFetcher,
}

impl Artifacts {
    /// Creates a store backed by `storage`, keeping blobs under `store_root`.
    pub fn new(storage: Storage, store_root: PathBuf) -> Self {
        Self {
            storage,
            blobs: BlobStore::new(store_root),
            oci: OciFetcher::new(),
        }
    }

    /// Read access to the storage catalog.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Fetches the `media_type` layer of the OCI image at `source` into the
    /// store and records it in the catalog under `name`. Every attempt, success
    /// or failure, is logged to the download history.
    pub async fn pull_oci(
        &self,
        name: &str,
        kind: ArtifactKind,
        source: &str,
        media_type: &MediaType,
    ) -> Result<Artifact> {
        let reference: Reference = source.parse().map_err(|err| {
            ArtifactError::Fetch(anyhow::format_err!("invalid reference {source:?}: {err}"))
        })?;
        let started = Timestamp::now();
        let repository = self.storage.artifacts();

        match self.oci.fetch(&reference, media_type, &self.blobs).await {
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
                Ok(artifact)
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
