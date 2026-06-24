//! Artifact manager for the Aperture gateway.
//!
//! Fetches and caches components (OCI images, host tools) into a
//! content-addressed blob store, recording each fetch in the storage catalog.

pub use self::artifacts::{Artifacts, SyncReport};
pub use self::blob::{BlobStore, Digest};
pub use self::error::{ArtifactError, Result};
pub use self::fetch::{Fetched, OciFetcher};
pub use self::media_type::MediaType;

mod artifacts;
mod blob;
mod error;
mod fetch;
mod media_type;
