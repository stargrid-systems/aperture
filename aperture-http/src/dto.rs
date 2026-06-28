//! Response and query types for the JSON API.
//!
//! List endpoints share one envelope ([`Page`]) and one set of pagination
//! query params ([`PageParams`]). Filtering and sorting params are added per
//! resource.

use aperture_artifacts::{Artifact, ArtifactKey, ListQuery, Order, Page as StoragePage, VersionSort};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Version information returned by `GET /api/v1/version`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VersionResponse {
    /// Version of the Aperture gateway.
    pub aperture: String,
}

impl VersionResponse {
    /// Builds a response reporting the given gateway version.
    pub fn new(version: &str) -> Self {
        Self {
            aperture: version.to_owned(),
        }
    }
}

/// A page of results plus the cursors for the neighbouring pages.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Page<T> {
    /// The rows in this page.
    pub items: Vec<T>,
    /// Cursor to pass as `?cursor=` for the next page. Null at the end.
    pub next_cursor: Option<String>,
    /// Cursor to pass as `?cursor=` for the previous page. Null at the start.
    pub prev_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Maps a storage page into a response page.
    fn from_storage<S>(page: StoragePage<S>, map: impl Fn(S) -> T) -> Self {
        Self {
            next_cursor: page.next_cursor,
            prev_cursor: page.prev_cursor,
            items: page.items.into_iter().map(map).collect(),
        }
    }
}

/// A distinct artifact key with its newest version, for `GET /api/v1/artifacts`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ArtifactSummaryResponse {
    /// Logical artifact key.
    pub key: String,
    /// How many versions of this key are stored.
    pub version_count: i64,
    /// Where the newest version came from.
    pub source: String,
    /// Content digest of the newest version.
    pub digest: String,
    /// Human-readable version of the newest version, if known.
    pub version: Option<String>,
    /// Stored blob size of the newest version, in bytes.
    pub size_bytes: i64,
    /// When the newest version was downloaded.
    pub downloaded_at: Timestamp,
}

impl From<ArtifactKey> for ArtifactSummaryResponse {
    fn from(key: ArtifactKey) -> Self {
        let latest = key.latest;
        Self {
            key: latest.key,
            version_count: key.version_count,
            source: latest.source,
            digest: latest.digest,
            version: latest.version,
            size_bytes: latest.size_bytes,
            downloaded_at: latest.downloaded_at,
        }
    }
}

/// One stored version of an artifact.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ArtifactVersionResponse {
    /// Logical artifact key.
    pub key: String,
    /// Content digest of the stored blob.
    pub digest: String,
    /// Where this version came from.
    pub source: String,
    /// Human-readable version, if known.
    pub version: Option<String>,
    /// OCI media type, if applicable.
    pub media_type: Option<String>,
    /// Stored blob size in bytes.
    pub size_bytes: i64,
    /// When this version was downloaded.
    pub downloaded_at: Timestamp,
    /// When this version was last verified, if ever.
    pub verified_at: Option<Timestamp>,
}

impl From<Artifact> for ArtifactVersionResponse {
    fn from(artifact: Artifact) -> Self {
        Self {
            key: artifact.key,
            digest: artifact.digest,
            source: artifact.source,
            version: artifact.version,
            media_type: artifact.media_type,
            size_bytes: artifact.size_bytes,
            downloaded_at: artifact.downloaded_at,
            verified_at: artifact.verified_at,
        }
    }
}

/// Sort direction shared by list endpoints.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OrderParam {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl From<OrderParam> for Order {
    fn from(order: OrderParam) -> Self {
        match order {
            OrderParam::Asc => Self::Asc,
            OrderParam::Desc => Self::Desc,
        }
    }
}

/// Query params for `GET /api/v1/artifacts`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct ArtifactListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction.
    pub order: Option<OrderParam>,
    /// Match keys containing this substring.
    pub q: Option<String>,
}

impl ArtifactListParams {
    pub(crate) fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}

/// Field a version listing is sorted by.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum VersionSortParam {
    /// When the version was downloaded.
    DownloadedAt,
    /// Stored blob size.
    SizeBytes,
}

impl From<VersionSortParam> for VersionSort {
    fn from(sort: VersionSortParam) -> Self {
        match sort {
            VersionSortParam::DownloadedAt => Self::DownloadedAt,
            VersionSortParam::SizeBytes => Self::SizeBytes,
        }
    }
}

/// Query params for `GET /api/v1/artifacts/{key}/versions`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct VersionListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction.
    pub order: Option<OrderParam>,
    /// Field to sort by. Defaults to downloaded time.
    pub sort: Option<VersionSortParam>,
    /// Only versions with this exact media type.
    pub media_type: Option<String>,
    /// Only versions with this exact version string.
    pub version: Option<String>,
}

impl VersionListParams {
    pub(crate) fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }

    pub(crate) fn sort(&self) -> VersionSort {
        self.sort.map(Into::into).unwrap_or(VersionSort::DownloadedAt)
    }
}

/// Maps a storage page of keys into the response envelope.
pub(crate) fn artifact_page(page: StoragePage<ArtifactKey>) -> Page<ArtifactSummaryResponse> {
    Page::from_storage(page, ArtifactSummaryResponse::from)
}

/// Maps a storage page of versions into the response envelope.
pub(crate) fn version_page(page: StoragePage<Artifact>) -> Page<ArtifactVersionResponse> {
    Page::from_storage(page, ArtifactVersionResponse::from)
}
