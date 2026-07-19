//! Artifact manager for the Aperture gateway.
//!
//! Fetches and caches components (OCI images, host tools) into a
//! content-addressed blob store, recording each fetch in the storage catalog.
//! Downloading is driven by the task system through [`DownloadDefinition`].

pub use aperture_storage::{
    Artifact, ArtifactKey, ArtifactKeyEntry, InvalidArtifactKey, ListQuery, Order, Page, Storage,
    StorageError, VersionSort,
};

pub use self::artifacts::{Artifacts, FetchRequest, FetchSource, Located, SyncReport};
pub use self::blob::BlobStore;
pub use self::change::{ArtifactChange, ChangeKind};
pub use self::digest::{Digest, DigestAlgorithm};
pub use self::download::{DownloadDefinition, DownloadInput, DownloadOutput, DownloadSource};
pub use self::error::{ArtifactError, Result};
pub use self::media_type::{InvalidMediaType, MediaType};

mod artifacts;
mod blob;
mod change;
mod digest;
mod download;
mod error;
mod fetch;
mod hash_writer;
mod media_type;
mod progress;
