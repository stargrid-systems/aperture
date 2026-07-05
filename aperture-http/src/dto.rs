//! Response and query types for the JSON API.
//!
//! List endpoints share one envelope ([`Page`]) and one sort direction enum
//! ([`OrderParam`]). Domain-specific DTOs live in submodules:
//!
//! - [`artifact`]: artifact catalog responses and query params
//! - [`task`]: task invocation responses, progress, and query params
//! - [`log`]: log event and span responses, boot info, and query params

pub(crate) mod artifact;
pub(crate) mod log;
pub(crate) mod task;

use aperture_artifacts::Page as StoragePage;
pub use artifact::{
    ArtifactListParams, ArtifactSummaryResponse, ArtifactVersionResponse, VersionListParams,
    VersionSortParam, artifact_page, version_page,
};
pub use log::{
    BootResponse, LevelResponse, LogEventResponse, LogListParams, LogSpanDetailResponse,
    LogSpanListParams, LogSpanResponse, LogTargetListParams, boots_response, event_page, span_page,
};
use serde::{Deserialize, Serialize};
pub use task::{
    CreateTaskRequest, TaskDefinitionResponse, TaskListParams, TaskResponse, TaskStatusParam,
    task_page,
};
use utoipa::ToSchema;
use uuid::Uuid;

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
