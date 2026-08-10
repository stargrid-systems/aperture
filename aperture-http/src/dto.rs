//! Response and query types for the JSON API.

use std::collections::{BTreeMap, HashMap};

use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

pub use self::artifact::{
    ArtifactListParams, ArtifactSummaryResponse, ArtifactVersionResponse, VersionListParams,
    VersionSortParam,
};
pub use self::log::{
    BootResponse, LevelResponse, LogEventResponse, LogListParams, LogSpanDetailResponse,
    LogSpanListParams, LogSpanResponse, LogTargetListParams,
};
pub use self::page::Page;
pub use self::task::{
    CreateTaskRequest, TaskDefinitionResponse, TaskListParams, TaskResponse, TaskStatusParam,
};
pub use self::task_schedule::{
    CreateTaskScheduleRequest, TaskScheduleListParams, TaskScheduleResponse,
    UpdateTaskScheduleRequest,
};

mod artifact;
mod log;
mod page;
mod task;
mod task_schedule;

/// Deserializes either a single comma-separated string or a sequence of
/// strings into a `Vec<String>`.
///
/// Accepts `target=A,B` (single param, comma separated) and
/// `target=A&target=B` (repeated param) forms, as well as a single value
/// `target=A`. Empty values produce an empty `Vec`.
pub fn deserialize_single_or_vec_string<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
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
/// query parameter, e.g. `{"key":"value"}`.
///
/// The raw string is parsed into key-value pairs during deserialization.
#[derive(Debug, Clone, Default, ToSchema)]
#[schema(value_type = String, example = "{\"status\":\"ok\"}")]
pub struct JsonQueryString(pub BTreeMap<String, String>);

impl JsonQueryString {
    pub fn into_pairs(self) -> Vec<(String, String)> {
        self.0.into_iter().collect()
    }
}

impl<'de> Deserialize<'de> for JsonQueryString {
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

/// Pagination query parameters for endpoints with no extra filters.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct SimpleListParams {
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub order: Option<OrderParam>,
}

impl SimpleListParams {
    /// Converts these params into a storage `ListQuery`.
    pub fn to_query(&self) -> aperture_artifacts::ListQuery {
        aperture_artifacts::ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}
