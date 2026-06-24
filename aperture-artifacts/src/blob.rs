//! Content-addressed blob store.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fmt, process};

use sha2::{Digest as _, Sha256};
use tokio::fs;
use tokio::io::{self, AsyncBufReadExt, AsyncRead, AsyncWriteExt as _};

use crate::error::{ArtifactError, Result};

const ALGORITHM: &str = "sha256";
const CHUNK: usize = 64 * 1024;
const HEX_LEN: usize = 64;

/// A content digest (sha256).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Digest {
    hex: Box<str>,
}

impl Digest {
    fn from_hash(hash: &[u8]) -> Self {
        let hex = hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            .into_boxed_str();
        Self { hex }
    }

    /// Returns the hex digest without the algorithm prefix.
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl FromStr for Digest {
    type Err = ArtifactError;

    /// Parses a digest of the form `sha256:<hex>`.
    fn from_str(value: &str) -> Result<Self> {
        let (algorithm, hex) = value
            .split_once(':')
            .ok_or_else(|| ArtifactError::InvalidDigest(value.to_owned()))?;
        let valid = algorithm == ALGORITHM
            && hex.len() == HEX_LEN
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            return Err(ArtifactError::InvalidDigest(value.to_owned()));
        }
        Ok(Self {
            hex: hex.to_ascii_lowercase().into_boxed_str(),
        })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{ALGORITHM}:{}", self.hex)
    }
}

/// A content-addressed store of blobs on disk, under a single root directory.
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Creates a store rooted at `root`. Directories are created on first
    /// write.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn blob_dir(&self) -> PathBuf {
        self.root.join("blobs").join(ALGORITHM)
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// The absolute path of the blob with `digest`.
    pub fn path(&self, digest: &Digest) -> PathBuf {
        self.blob_dir().join(digest.hex())
    }

    /// The path of the blob relative to the store root.
    pub fn relative_path(&self, digest: &Digest) -> PathBuf {
        Path::new("blobs").join(ALGORITHM).join(digest.hex())
    }

    /// Returns whether the blob with `digest` is already stored.
    pub async fn contains(&self, digest: &Digest) -> bool {
        fs::try_exists(self.path(digest)).await.unwrap_or(false)
    }

    /// Lists the digests of all stored blobs.
    pub async fn list(&self) -> Result<Vec<Digest>> {
        let mut entries = match fs::read_dir(self.blob_dir()).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut digests = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Ok(digest) = format!("{ALGORITHM}:{name}").parse::<Digest>() {
                digests.push(digest);
            }
        }
        Ok(digests)
    }

    /// Removes the blob with `digest`. Does nothing if it is absent.
    pub async fn remove(&self, digest: &Digest) -> Result<()> {
        remove_if_exists(self.path(digest)).await
    }

    /// Removes any leftover temporary files from interrupted writes.
    pub async fn clear_temp(&self) -> Result<()> {
        let mut entries = match fs::read_dir(self.tmp_dir()).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            remove_if_exists(entry.path()).await?;
        }
        Ok(())
    }

    /// Creates an empty temporary file to stream a blob into, returning its
    /// path and an open handle. Finish the write and store it with
    /// [`BlobStore::commit`].
    pub async fn temp_file(&self) -> Result<(PathBuf, fs::File)> {
        fs::create_dir_all(self.tmp_dir()).await?;
        let path = self.tmp_dir().join(tmp_name());
        let file = fs::File::create(&path).await?;
        Ok((path, file))
    }

    /// Hashes the file at `temp`, moves it into the store under its digest, and
    /// returns the digest and byte length.
    pub async fn commit(&self, temp: &Path) -> Result<(Digest, u64)> {
        let file = fs::File::open(temp).await?;
        let mut file = io::BufReader::with_capacity(CHUNK, file);
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        loop {
            let buf = file.fill_buf().await?;
            if buf.is_empty() {
                break;
            }
            hasher.update(buf);
            let read = buf.len();
            total += read as u64;
            file.consume(read);
        }
        drop(file);

        let hash = hasher.finalize();
        let digest = Digest::from_hash(&hash);
        fs::create_dir_all(self.blob_dir()).await?;
        fs::rename(temp, self.path(&digest)).await?;
        Ok((digest, total))
    }

    /// Streams `reader` into the store. Returns the content digest and length.
    pub async fn put<R>(&self, mut reader: R) -> Result<(Digest, u64)>
    where
        R: AsyncRead + Unpin,
    {
        let (temp, mut file) = self.temp_file().await?;
        io::copy(&mut reader, &mut file).await?;
        file.flush().await?;
        drop(file);
        self.commit(&temp).await
    }
}

async fn remove_if_exists(path: PathBuf) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn tmp_name() -> String {
    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{pid}-{counter}.tmp", pid = process::id())
}
