//! Artifact manager for the Aperture gateway.
//!
//! Fetches and caches components (OCI images, host tools) into a
//! content-addressed blob store, recording each fetch in the storage catalog
//! and tracking downloads that are in flight.

pub use aperture_storage::{
    Artifact, ArtifactKey, Download, DownloadStatus, ListQuery, Order, Page, Storage, StorageError,
    VersionSort,
};
pub use aperture_storage::{
    Event, EventFilter, EventInsertBuilder, EventRecord, Level, LogRepository, PreparedStatements,
    Span, SpanFilter, SpanInsertBuilder, SpanRecord,
};

pub use self::artifacts::{
    Artifacts, DownloadHandle, FetchRequest, FetchSource, Located, SyncReport,
};
pub use self::blob::BlobStore;
pub use self::digest::{Digest, DigestAlgorithm};
pub use self::downloads::DownloadProgress;
pub use self::error::{ArtifactError, Result};
pub use self::media_type::MediaType;

mod artifacts;
mod blob;
mod digest;
mod downloads;
mod error;
mod fetch;
mod hash_writer;
mod media_type;
