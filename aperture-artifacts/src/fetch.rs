//! Fetching artifacts into the blob store.

pub use self::oci::OciFetcher;
use crate::blob::Digest;
use crate::media_type::MediaType;

mod oci;

/// The result of a successful fetch.
pub struct Fetched {
    /// Content digest of the stored blob.
    pub digest: Digest,
    /// The fetched layer's media type.
    pub media_type: MediaType,
    /// Number of bytes stored.
    pub size: u64,
}
