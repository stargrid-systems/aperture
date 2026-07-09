//! Response and query types for the JSON API.

use std::collections::HashMap;

use aperture_artifacts::Page as StoragePage;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

pub use self::artifact::{
    ArtifactListParams, ArtifactSummaryResponse, ArtifactVersionResponse, VersionListParams,
    VersionSortParam, artifact_page, version_page,
};
pub use self::log::{
    BootResponse, LevelResponse, LogEventResponse, LogListParams, LogSpanDetailResponse,
    LogSpanListParams, LogSpanResponse, LogTargetListParams, boots_response, event_page, span_page,
};
pub use self::task::{
    CreateTaskRequest, TaskDefinitionResponse, TaskListParams, TaskResponse, TaskStatusParam,
    task_page,
};

mod artifact;
mod log;
mod task;

/// Deserializes either a single comma-separated string or a sequence of
/// strings into a `Vec<String>`. Accepts `target=A,B` (single param, comma
/// separated) and `target=A&target=B` (repeated param) forms, as well as a
/// single value `target=A`. Empty values produce an empty `Vec`.
pub(crate) fn deserialize_single_or_vec_string<'de, D>(
    deserializer: D,
) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => s
            .split(',')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect(),
        OneOrMany::Many(v) => v,
    })
}

/// A structured field filter passed as a string-encoded JSON object in a
/// query parameter, e.g. `{"key":"value"}`. The raw string is parsed into
/// key-value pairs during deserialization.
#[derive(Debug, Clone, Default)]
pub struct FieldFilter(pub Vec<(String, String)>);

impl FieldFilter {
    pub fn into_pairs(self) -> Vec<(String, String)> {
        self.0
    }
}

impl<'de> Deserialize<'de> for FieldFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let map: HashMap<String, String> = serde_json::from_str(&raw).map_err(D::Error::custom)?;
        Ok(Self(map.into_iter().collect()))
    }
}

/// Version information returned by `GET /api/v1/version`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VersionResponse {
    /// Version of the Aperture gateway.
    pub aperture: String,
    /// Unique id of this gateway boot session.
    pub boot_id: Uuid,
}

impl VersionResponse {
    /// Builds a response reporting the given gateway version and boot id.
    pub fn new(version: &str, boot_id: Uuid) -> Self {
        Self {
            aperture: version.to_owned(),
            boot_id,
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
    pub fn from_storage<S>(page: StoragePage<S>, map: impl Fn(S) -> T) -> Self {
        Self {
            next_cursor: page.next_cursor,
            prev_cursor: page.prev_cursor,
            items: page.items.into_iter().map(map).collect(),
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

impl From<OrderParam> for aperture_artifacts::Order {
    fn from(order: OrderParam) -> Self {
        match order {
            OrderParam::Asc => Self::Asc,
            OrderParam::Desc => Self::Desc,
        }
    }
}
