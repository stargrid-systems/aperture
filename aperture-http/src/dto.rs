//! Response and query types for the JSON API.

use aperture_artifacts::Page as StoragePage;
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
