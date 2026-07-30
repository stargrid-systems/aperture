//! The artifacts store: fetches artifacts into the blob store, records them in
//! the storage catalog, and keeps the two consistent.
//!
//! Downloading is exposed as a single [`Artifacts::download`] call. The task
//! system (see [`crate::DownloadDefinition`]) drives it, tracks progress, and
//! records each invocation. This layer just does the work.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use aperture_storage::{
    Artifact, ArtifactId, ArtifactKey, ArtifactKeyEntry, Digest, ListQuery, MediaType, Page,
    Storage, VersionSort,
};
use aperture_tasks::ProgressHandle;
use jiff::{SignedDuration, Timestamp};
use oci_client::Reference;
use tokio::fs;
use tokio::io::AsyncRead;
use tokio::sync::{Mutex as AsyncMutex, broadcast};

use crate::blob::BlobStore;
use crate::change::{ArtifactChange, ChangeKind};
use crate::error::{ArtifactError, Result};
use crate::fetch::{FetchMeta, Fetched, OciFetcher, Resolved};
use crate::hash_writer::HashWriter;
use crate::progress::ProgressWriter;

/// Capacity of the in-process change feed.
const CHANGE_FEED_CAPACITY: usize = 64;

/// How long a resolved reference stays cached before it is re-checked against
/// the registry.
const RESOLUTION_TTL: SignedDuration = SignedDuration::from_mins(5);

/// What an [`Artifacts::sync`] run removed.
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
    pub key: ArtifactKey,
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

    /// The key a resolution is cached under. Two requests that resolve the same
    /// layer share it, so it covers everything that picks the layer.
    fn cache_key(&self) -> String {
        match &self.source {
            FetchSource::Oci {
                reference,
                media_type,
            } => format!("oci\0{reference}\0{}", media_type.as_str()),
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

/// A resolution held in the cache, with when it was made.
struct CachedResolution {
    resolved: Resolved,
    resolved_at: Timestamp,
}

struct Inner {
    storage: Storage,
    blobs: BlobStore,
    oci: OciFetcher,
    /// Recent reference resolutions, so repeated downloads skip the manifest
    /// lookup for up to [`RESOLUTION_TTL`].
    resolutions: Mutex<HashMap<String, CachedResolution>>,
    /// One lock per content digest, so concurrent downloads of the same content
    /// collapse onto a single transfer instead of each pulling it.
    pull_locks: Mutex<HashMap<Digest, Weak<AsyncMutex<()>>>>,
    /// Best-effort feed of artifact changes. See [`ArtifactChange`].
    changes: broadcast::Sender<ArtifactChange>,
}

impl Artifacts {
    /// Creates a store backed by `storage`, keeping blobs under `store_root`.
    pub fn new(storage: Storage, store_root: PathBuf) -> Self {
        let (changes, _) = broadcast::channel(CHANGE_FEED_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                storage,
                blobs: BlobStore::new(store_root),
                oci: OciFetcher::new(),
                resolutions: Mutex::new(HashMap::new()),
                pull_locks: Mutex::new(HashMap::new()),
                changes,
            }),
        }
    }

    /// Subscribes to artifact changes.
    pub fn subscribe(&self) -> broadcast::Receiver<ArtifactChange> {
        self.inner.changes.subscribe()
    }

    /// Returns the blob of the newest stored version of `key`, if present on
    /// disk.
    pub async fn locate(&self, key: &ArtifactKey) -> Result<Option<Located>> {
        self.inner.locate(key).await
    }

    /// Returns the blob of the `(key, digest)` version, if present on disk.
    pub async fn locate_version(
        &self,
        key: &ArtifactKey,
        digest: &Digest,
    ) -> Result<Option<Located>> {
        self.inner.locate_version(key, digest).await
    }

    /// Lists stored artifact keys with their newest version and version count.
    /// `q` matches a substring of the key.
    pub async fn list_artifacts(
        &self,
        q: Option<&str>,
        query: &ListQuery,
    ) -> Result<Page<ArtifactKeyEntry>> {
        Ok(self.inner.storage.artifacts()?.list_keys(q, query).await?)
    }

    /// Returns one artifact key with its newest version and version count.
    pub async fn artifact(&self, key: &ArtifactKey) -> Result<Option<ArtifactKeyEntry>> {
        Ok(self.inner.storage.artifacts()?.get_key(key).await?)
    }

    /// Lists the stored versions of `key`, optionally filtered by exact
    /// `media_type` and `version`.
    pub async fn list_versions(
        &self,
        key: &ArtifactKey,
        sort: VersionSort,
        media_type: Option<&MediaType>,
        version: Option<&str>,
        query: &ListQuery,
    ) -> Result<Page<Artifact>> {
        Ok(self
            .inner
            .storage
            .artifacts()?
            .list_versions(key, sort, media_type, version, query)
            .await?)
    }

    /// Returns the `(key, digest)` version, if stored.
    pub async fn version(&self, key: &ArtifactKey, digest: &Digest) -> Result<Option<Artifact>> {
        Ok(self
            .inner
            .storage
            .artifacts()?
            .get_version(key, digest)
            .await?)
    }

    /// Removes the `(key, digest)` version, and its blob if no other version
    /// references it. Returns whether the version existed.
    pub async fn evict_version(&self, key: &ArtifactKey, digest: &Digest) -> Result<bool> {
        let removed = self.inner.evict_version(key, digest).await?;
        if removed {
            self.inner.notify(ArtifactChange {
                key: key.clone(),
                kind: ChangeKind::Removed,
                digest: None,
            });
        }
        Ok(removed)
    }

    /// Stores `reader` as a content-addressed blob and records it under `key`.
    ///
    /// Returns the stored version. The `digest` field of the returned artifact
    /// is always a valid `Digest` (i.e. `artifact.digest.parse::<Digest>()`
    /// succeeds). The same guarantee holds for any `Artifact` read back from
    /// the catalog.
    ///
    /// `media_type` is stored verbatim. Callers must validate upstream if they
    /// need to reject invalid values.
    pub async fn put<R>(
        &self,
        key: &ArtifactKey,
        media_type: Option<&MediaType>,
        reader: R,
    ) -> Result<Artifact>
    where
        R: AsyncRead + Unpin,
    {
        let (digest, size) = self.inner.blobs.put(reader).await?;
        let now = Timestamp::now();
        let artifact = Artifact {
            id: ArtifactId::from(0),
            key: key.clone(),
            source: "upload".to_owned(),
            digest: digest.clone(),
            media_type: media_type.cloned(),
            version: None,
            size_bytes: size,
            downloaded_at: now,
            verified_at: Some(now),
        };
        self.inner
            .storage
            .artifacts()?
            .record_version(&artifact)
            .await?;
        self.inner.notify(ArtifactChange {
            key: key.clone(),
            kind: ChangeKind::Written,
            digest: Some(digest.clone()),
        });
        Ok(artifact)
    }

    /// Ensures the artifact in `request` is present, downloading it if needed,
    /// and returns the stored version.
    ///
    /// If the newest version is already on disk it is returned without
    /// fetching. Transferred bytes are reported into `progress`.
    pub async fn download(
        &self,
        request: FetchRequest,
        progress: ProgressHandle,
    ) -> Result<Artifact> {
        let (artifact, written) = self.inner.download(&request, &progress).await?;
        if written {
            self.inner.notify(ArtifactChange {
                key: artifact.key.clone(),
                kind: ChangeKind::Written,
                digest: Some(artifact.digest.clone()),
            });
        }
        Ok(artifact)
    }

    /// Reconciles the catalog with the blob store.
    ///
    /// Removes catalog entries whose blob is missing, removes blobs that no
    /// entry references, and clears leftover temporary files.
    pub async fn sync(&self) -> Result<SyncReport> {
        self.inner.sync().await
    }
}

impl Inner {
    /// Publishes `change` to the feed.
    ///
    /// Late or lagging receivers are dropped. A send error here is expected
    /// and silently ignored.
    fn notify(&self, change: ArtifactChange) {
        let _ = self.changes.send(change);
    }

    async fn locate(&self, key: &ArtifactKey) -> Result<Option<Located>> {
        match self.storage.artifacts()?.latest(key).await? {
            Some(artifact) => self.locate_digest(&artifact.digest).await,
            None => Ok(None),
        }
    }

    async fn locate_version(&self, key: &ArtifactKey, digest: &Digest) -> Result<Option<Located>> {
        match self.storage.artifacts()?.get_version(key, digest).await? {
            Some(artifact) => self.locate_digest(&artifact.digest).await,
            None => Ok(None),
        }
    }

    async fn locate_digest(&self, digest: &Digest) -> Result<Option<Located>> {
        if !self.blobs.contains(digest).await {
            return Ok(None);
        }
        Ok(Some(Located {
            path: self.blobs.path(digest),
            digest: digest.clone(),
        }))
    }

    /// Removes a stored version, and its blob when no other version still
    /// references that digest. Returns whether the version existed.
    async fn evict_version(&self, key: &ArtifactKey, digest: &Digest) -> Result<bool> {
        let repository = self.storage.artifacts()?;
        let Some(artifact) = repository.get_version(key, digest).await? else {
            return Ok(false);
        };
        repository.delete_version(key, digest).await?;

        let still_referenced = repository
            .all_versions()
            .await?
            .iter()
            .any(|version| version.digest == artifact.digest);
        if !still_referenced {
            self.blobs.remove(&artifact.digest).await?;
        }
        Ok(true)
    }

    /// Resolves the request to a content digest, reuses the blob if it is
    /// already present, and otherwise fetches it.
    ///
    /// Returns the artifact and whether a new version was recorded (and thus
    /// whether a change-feed notification is warranted).
    async fn download(
        &self,
        request: &FetchRequest,
        progress: &ProgressHandle,
    ) -> Result<(Artifact, bool)> {
        let repository = self.storage.artifacts()?;
        let now = Timestamp::now();
        let resolved = self.resolve(request, now).await?;

        // Fast path: this version is already recorded and its blob is present.
        // Read-only, so no lock is needed.
        if let Some(existing) = repository
            .get_version(&request.key, &resolved.digest)
            .await?
            && self.blobs.contains(&resolved.digest).await
        {
            return Ok((existing, false));
        }

        // Anything that writes runs under the per-digest lock, so concurrent
        // callers for the same content serialize instead of racing.
        let lock = self.pull_lock(&resolved.digest);
        let _guard = lock.lock().await;

        // Re-check under the lock: another caller may have finished meanwhile.
        if let Some(existing) = repository
            .get_version(&request.key, &resolved.digest)
            .await?
        {
            return Ok((existing, false));
        }
        // The blob is present (shared from another key), so record without
        // pulling.
        if self.blobs.contains(&resolved.digest).await {
            let artifact = build_artifact(
                request,
                &resolved.digest,
                &resolved.media_type,
                resolved.version.clone(),
                resolved.size,
                now,
            );
            repository.record_version(&artifact).await?;
            return Ok((artifact, true));
        }

        let fetched = self.execute(request, progress).await?;
        let artifact = build_artifact(
            request,
            &fetched.digest,
            &fetched.media_type,
            fetched.version.clone(),
            fetched.size,
            Timestamp::now(),
        );
        repository.record_version(&artifact).await?;
        Ok((artifact, true))
    }

    /// Resolves `request` to its content, using the cache when a recent entry
    /// exists and refreshing it otherwise.
    async fn resolve(&self, request: &FetchRequest, now: Timestamp) -> Result<Resolved> {
        let key = request.cache_key();
        if let Some(resolved) = self.cached_resolution(&key, now) {
            return Ok(resolved);
        }
        let resolved = match &request.source {
            FetchSource::Oci {
                reference,
                media_type,
            } => {
                let reference = parse_reference(reference)?;
                self.oci.resolve(&reference, media_type).await?
            }
        };
        self.resolutions
            .lock()
            .expect("resolutions poisoned")
            .insert(
                key,
                CachedResolution {
                    resolved: resolved.clone(),
                    resolved_at: now,
                },
            );
        Ok(resolved)
    }

    /// A cached resolution for `key`, if one exists and has not expired.
    fn cached_resolution(&self, key: &str, now: Timestamp) -> Option<Resolved> {
        let cache = self.resolutions.lock().expect("resolutions poisoned");
        let entry = cache.get(key)?;
        is_fresh(entry.resolved_at, now).then(|| entry.resolved.clone())
    }

    /// The lock guarding downloads of `digest`, shared across callers. A dead
    /// entry is replaced, so the map only holds locks that are still in use.
    fn pull_lock(&self, digest: &Digest) -> Arc<AsyncMutex<()>> {
        let mut locks = self.pull_locks.lock().expect("pull_locks poisoned");
        if let Some(lock) = locks.get(digest).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        locks.insert(digest.clone(), Arc::downgrade(&lock));
        lock
    }

    /// Dispatches a fetch to the right fetcher for its source.
    async fn execute(&self, request: &FetchRequest, progress: &ProgressHandle) -> Result<Fetched> {
        match &request.source {
            FetchSource::Oci {
                reference,
                media_type,
            } => {
                let reference = parse_reference(reference)?;
                self.fetch_oci(&reference, media_type, progress).await
            }
        }
    }

    /// Stages an OCI fetch into the blob store: streams the layer into a
    /// temporary file, verifies the bytes against the advertised digest, then
    /// places the blob under its digest. The fetcher only ever sees the sink,
    /// so it cannot reach the store on its own.
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

        let repository = self.storage.artifacts()?;
        let mut report = SyncReport::default();
        let mut tracked: HashSet<Digest> = HashSet::new();

        for artifact in repository.all_versions().await? {
            if self.blobs.contains(&artifact.digest).await {
                tracked.insert(artifact.digest.clone());
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

/// Parses an OCI reference string, attaching context on failure.
fn parse_reference(reference: &str) -> Result<Reference> {
    reference.parse().map_err(|err| {
        ArtifactError::Fetch(
            anyhow::Error::from(err).context(format!("invalid reference {reference:?}")),
        )
    })
}

/// Whether a resolution made at `resolved_at` is still fresh at `now`.
fn is_fresh(resolved_at: Timestamp, now: Timestamp) -> bool {
    now.duration_since(resolved_at) < RESOLUTION_TTL
}

/// Builds a version record. Content is digest-addressed, so `at` stamps both
/// the download and the verification: a present blob is proof enough.
fn build_artifact(
    request: &FetchRequest,
    digest: &Digest,
    media_type: &MediaType,
    version: Option<String>,
    size: u64,
    at: Timestamp,
) -> Artifact {
    Artifact {
        id: ArtifactId::from(0),
        key: request.key.clone(),
        source: request.source_str().to_owned(),
        digest: digest.clone(),
        media_type: Some(media_type.clone()),
        version,
        size_bytes: size,
        downloaded_at: at,
        verified_at: Some(at),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;
    use std::{env, fs, process};

    use aperture_storage::Storage;
    use tokio::time::timeout;

    use super::*;

    fn at(micros: i64) -> Timestamp {
        Timestamp::from_microsecond(micros).unwrap()
    }

    fn oci_request(reference: &str, media_type: &str) -> FetchRequest {
        FetchRequest {
            key: ArtifactKey::new("spectra").expect("valid key"),
            source: FetchSource::Oci {
                reference: reference.to_owned(),
                media_type: media_type.parse().expect("valid media type"),
            },
        }
    }

    /// Builds an in-memory artifacts store rooted at a fresh temp dir.
    async fn fresh_store() -> (Artifacts, PathBuf) {
        let dir = env::temp_dir().join(format!(
            "aperture-artifacts-tests-{}-{}",
            process::id(),
            uuid::Uuid::new_v4()
        ));
        let storage = Storage::open(":memory:").await.unwrap();
        let artifacts = Artifacts::new(storage, dir.clone());
        (artifacts, dir)
    }

    /// Removes the temp dir created by [`fresh_store`]. Best-effort.
    fn cleanup(dir: PathBuf) {
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_publishes_written_event() {
        let (artifacts, dir) = fresh_store().await;
        let mut rx = artifacts.subscribe();
        let key = ArtifactKey::new("firmware").unwrap();
        let artifact = artifacts
            .put(
                &key,
                Some(&"application/octet-stream".parse().unwrap()),
                &b"bytes"[..],
            )
            .await
            .unwrap();
        let change = rx.recv().await.expect("feed emitted an event");
        assert_eq!(change.key, key);
        assert_eq!(change.kind, ChangeKind::Written);
        assert_eq!(
            change.digest.map(|d| d.to_string()),
            Some(artifact.digest.to_string()),
            "Written events must carry the new digest",
        );
        cleanup(dir);
    }

    #[tokio::test]
    async fn put_records_media_type_verbatim() {
        // Artifacts::put no longer sanitises the media type. The HTTP layer
        // validates upstream via FromStr, so anything reaching the store is
        // already known good and is stored verbatim.
        let (artifacts, dir) = fresh_store().await;
        let key = ArtifactKey::new("firmware").unwrap();
        let artifact = artifacts
            .put(
                &key,
                Some(&"application/octet-stream".parse().unwrap()),
                &b"bytes2"[..],
            )
            .await
            .unwrap();
        assert_eq!(
            artifact.media_type.as_ref().map(|mt| mt.as_str()),
            Some("application/octet-stream")
        );

        cleanup(dir);
    }

    #[tokio::test]
    async fn evict_version_publishes_removed_event() {
        let (artifacts, dir) = fresh_store().await;
        let key = ArtifactKey::new("firmware").unwrap();
        let artifact = artifacts.put(&key, None, &b"bytes"[..]).await.unwrap();

        let mut rx = artifacts.subscribe();
        artifacts
            .evict_version(&key, &artifact.digest)
            .await
            .unwrap();
        let change = rx.recv().await.expect("feed emitted an event");
        assert_eq!(change.key, key);
        assert_eq!(change.kind, ChangeKind::Removed);
        assert!(
            change.digest.is_none(),
            "Removed events do not carry a digest"
        );
        cleanup(dir);
    }

    #[tokio::test]
    async fn late_subscriber_misses_earlier_events() {
        let (artifacts, dir) = fresh_store().await;
        let key = ArtifactKey::new("firmware").unwrap();
        artifacts.put(&key, None, &b"first"[..]).await.unwrap();

        // Subscribe after the write completed. No event should arrive.
        let mut rx = artifacts.subscribe();
        let outcome = timeout(Duration::from_millis(50), rx.recv()).await;
        assert!(
            outcome.is_err(),
            "late subscriber should not see prior events"
        );
        cleanup(dir);
    }

    #[test]
    fn resolution_is_fresh_within_the_ttl() {
        let base = at(1_000_000);
        let ttl_micros: i64 = RESOLUTION_TTL.as_micros().try_into().unwrap();
        assert!(is_fresh(base, base));
        assert!(is_fresh(base, at(1_000_000 + ttl_micros - 1)));
        assert!(!is_fresh(base, at(1_000_000 + ttl_micros)));
    }

    #[test]
    fn cache_key_separates_reference_and_media_type() {
        let a = oci_request("ghcr.io/x/spectra:1", "application/foo");
        let b = oci_request("ghcr.io/x/spectra:1", "application/bar");
        let c = oci_request("ghcr.io/x/spectra:2", "application/foo");
        assert_eq!(
            a.cache_key(),
            oci_request("ghcr.io/x/spectra:1", "application/foo").cache_key()
        );
        assert_ne!(a.cache_key(), b.cache_key());
        assert_ne!(a.cache_key(), c.cache_key());
    }
}
