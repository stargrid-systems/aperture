//! Error type for the artifact manager.

use std::io;
use std::result::Result as StdResult;

use aperture_storage::StorageError;

/// Errors returned by the artifact manager.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    /// A storage-layer error.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    /// Fetching the artifact failed.
    #[error("fetch failed: {0}")]
    Fetch(#[source] anyhow::Error),
    /// A filesystem error in the blob store.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// The downloaded content did not match the advertised digest.
    #[error("digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch {
        /// The digest the source advertised.
        expected: String,
        /// The digest computed from the downloaded bytes.
        actual: String,
    },
}

impl From<aperture_storage::InvalidDigest> for ArtifactError {
    fn from(err: aperture_storage::InvalidDigest) -> Self {
        StorageError::from(err).into()
    }
}

/// Result alias for the artifact manager.
pub type Result<T> = StdResult<T, ArtifactError>;
