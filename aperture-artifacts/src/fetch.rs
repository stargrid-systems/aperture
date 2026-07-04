//! Fetching artifact bytes from a source.
//!
//! A fetcher only streams bytes into a sink it is handed. It never touches the
//! blob store or the catalog, so it cannot store an artifact without the
//! [`Artifacts`](crate::Artifacts) layer placing and recording it.

pub use self::oci::OciFetcher;
use crate::digest::Digest;
use crate::media_type::MediaType;

mod oci;

/// What a source resolves to without transferring the blob. The `digest`
/// identifies the content, so a blob already on disk under it can be reused. The
/// rest describes the version to record.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Content digest the source advertises.
    pub digest: Digest,
    /// The resolved layer's media type.
    pub media_type: MediaType,
    /// The layer size in bytes.
    pub size: u64,
    /// Human-readable version, if the source carries one.
    pub version: Option<String>,
}

/// What a fetcher resolves about an artifact while streaming it into a sink.
pub struct FetchMeta {
    /// Digest the source advertises for the content.
    pub expected_digest: Digest,
    /// The fetched layer's media type.
    pub media_type: MediaType,
}

/// A fetched blob that has been stored and verified.
pub struct Fetched {
    /// Content digest of the stored blob.
    pub digest: Digest,
    /// The fetched layer's media type.
    pub media_type: MediaType,
    /// Human-readable version, if the source carries one.
    pub version: Option<String>,
    /// Number of bytes stored.
    pub size: u64,
}
