//! Artifact manager for the Aperture gateway.
//!
//! Fetches and caches components (OCI images, host tools) into a
//! content-addressed blob store, recording each fetch in the storage catalog.
//! Downloading is driven by the task system through [`DownloadDefinition`].

pub use aperture_storage::{
    Artifact, ArtifactKey, ListQuery, Order, Page, Storage, StorageError, VersionSort,
};

pub use self::artifacts::{Artifacts, FetchRequest, FetchSource, Located, SyncReport};
pub use self::blob::BlobStore;
pub use self::digest::{Digest, DigestAlgorithm};
pub use self::download::{DownloadDefinition, DownloadInput, DownloadOutput, DownloadSource};
pub use self::error::{ArtifactError, Result};
pub use self::media_type::MediaType;

mod artifacts;
mod blob;
mod digest;
mod download;
mod error;
mod fetch;
mod hash_writer;
mod media_type;
mod progress;
