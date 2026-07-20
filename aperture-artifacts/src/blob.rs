//! Content-addressed blob store.

use std::collections::HashSet;
use std::error::Error as StdError;
use std::io::ErrorKind;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::fs;
use tokio::io::{self, AsyncRead};

use crate::digest::{Digest, DigestAlgorithm};
use crate::error::Result;
use crate::hash_writer::HashWriter;

/// A content-addressed store of blobs on disk, under a single root directory.
pub struct BlobStore {
    root: PathBuf,
    /// Paths of temporary files currently being written.
    /// [`BlobStore::clear_temp`] leaves these alone so it never deletes an
    /// in-flight write.
    active: Arc<Mutex<HashSet<PathBuf>>>,
}

impl BlobStore {
    /// Creates a store rooted at `root`. Directories are created on first
    /// write.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn blobs_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// The absolute path of the blob with `digest`.
    pub fn path(&self, digest: &Digest) -> PathBuf {
        self.blobs_dir()
            .join(digest.algorithm().as_str())
            .join(digest.hex())
    }

    /// Returns whether the blob with `digest` is already stored.
    ///
    /// IO errors other than "file not found" are logged and treated as
    /// "not present". A misconfigured store should not crash the gateway, but
    /// operators will see the warning.
    pub async fn contains(&self, digest: &Digest) -> bool {
        match fs::try_exists(self.path(digest)).await {
            Ok(exists) => exists,
            Err(err) if err.kind() == ErrorKind::NotFound => false,
            Err(err) => {
                tracing::warn!(
                    error = &err as &dyn StdError,
                    "blob existence check failed, treating as missing"
                );
                false
            }
        }
    }

    /// Lists the digests of all stored blobs.
    ///
    /// Blobs live under `blobs/<algorithm>/<hex>`, so this walks both levels:
    /// one directory per digest algorithm, then the hashes within it.
    pub async fn list(&self) -> Result<Vec<Digest>> {
        let mut algorithms = match fs::read_dir(self.blobs_dir()).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut digests = Vec::new();
        while let Some(algorithm) = algorithms.next_entry().await? {
            let name = algorithm.file_name();
            let Some(name) = name.to_str() else { continue };
            let Ok(algorithm_name) = name.parse::<DigestAlgorithm>() else {
                continue;
            };
            let mut hashes = match fs::read_dir(algorithm.path()).await {
                Ok(entries) => entries,
                Err(err) if err.kind() == ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            };
            while let Some(hash) = hashes.next_entry().await? {
                let hex = hash.file_name();
                let Some(hex) = hex.to_str() else { continue };
                if let Ok(digest) = format!("{algorithm_name}:{hex}").parse::<Digest>() {
                    digests.push(digest);
                }
            }
        }
        Ok(digests)
    }

    /// Removes the blob with `digest`. Does nothing if it is absent.
    pub async fn remove(&self, digest: &Digest) -> Result<()> {
        remove_if_exists(&self.path(digest)).await
    }

    /// Removes leftover temporary files from interrupted writes.
    ///
    /// Temporary files of writes still in flight in this process are kept. Any
    /// other file in the temp directory is left over from a crashed run, or
    /// from a write that has already finished, so it is safe to remove.
    pub async fn clear_temp(&self) -> Result<()> {
        let mut entries = match fs::read_dir(self.tmp_dir()).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if self.is_active(&path) {
                continue;
            }
            remove_if_exists(&path).await?;
        }
        Ok(())
    }

    /// Creates an empty temporary file to stream a blob into. Write through the
    /// returned `TempFile`, then store it with [`BlobStore::place`].
    pub async fn temp_file(&self) -> Result<TempFile> {
        fs::create_dir_all(self.tmp_dir()).await?;
        let path = self.tmp_dir().join(tmp_name());
        // Register before the file exists so a concurrent clear_temp can never
        // observe the file on disk without also seeing it as active.
        self.register(path.clone());
        let file = match fs::File::create(&path).await {
            Ok(file) => file,
            Err(err) => {
                self.unregister(&path);
                return Err(err.into());
            }
        };
        Ok(TempFile {
            path,
            file,
            active: Arc::clone(&self.active),
        })
    }

    /// Moves the file at `temp` into the store under `digest`. The caller is
    /// responsible for ensuring the file's content matches `digest`.
    pub async fn place(&self, temp: &Path, digest: &Digest) -> Result<()> {
        let path = self.path(digest);
        if let Some(parent_dir) = path.parent() {
            fs::create_dir_all(parent_dir).await?;
        }
        fs::rename(temp, path).await?;
        Ok(())
    }

    /// Streams `reader` into the store. Returns the content digest and length.
    pub async fn put<R>(&self, mut reader: R) -> Result<(Digest, u64)>
    where
        R: AsyncRead + Unpin,
    {
        let mut temp = self.temp_file().await?;
        let write_result = async {
            let mut writer = HashWriter::new(&mut *temp);
            io::copy(&mut reader, &mut writer).await?;
            writer.finalize().await
        }
        .await;
        match write_result {
            Ok((digest, size)) => {
                self.place(temp.path(), &digest).await?;
                Ok((digest, size))
            }
            Err(err) => {
                let _ = remove_if_exists(temp.path()).await;
                Err(err.into())
            }
        }
    }

    fn register(&self, path: PathBuf) {
        self.active
            .lock()
            .expect("temp registry poisoned")
            .insert(path);
    }

    fn unregister(&self, path: &Path) {
        self.active
            .lock()
            .expect("temp registry poisoned")
            .remove(path);
    }

    fn is_active(&self, path: &Path) -> bool {
        self.active
            .lock()
            .expect("temp registry poisoned")
            .contains(path)
    }
}

/// A temporary file being streamed into a [`BlobStore`].
///
/// Derefs to the underlying [`fs::File`] for writing. While alive, its path is
/// registered as active so [`BlobStore::clear_temp`] will not remove it.
/// Dropping the handle unregisters the path, marking the file as removable. The
/// file itself is not deleted on drop, since that would block. A later
/// [`BlobStore::clear_temp`] reclaims it if it was never placed.
pub struct TempFile {
    path: PathBuf,
    file: fs::File,
    active: Arc<Mutex<HashSet<PathBuf>>>,
}

impl TempFile {
    /// The path of the temporary file on disk.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for TempFile {
    type Target = fs::File;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

impl DerefMut for TempFile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.file
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.path);
        }
    }
}

async fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn tmp_name() -> String {
    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{pid}-{counter}.tmp", pid = process::id())
}
