//! Response and query types for the JSON API.
//!
//! List endpoints share one envelope ([`Page`]) and one set of pagination
//! query params ([`PageParams`]). Filtering and sorting params are added per
//! resource.

use std::collections::HashMap;

use aperture_artifacts::{
    Artifact, ArtifactKey, ListQuery, Order, Page as StoragePage, VersionSort,
};
use aperture_tasks::{
    JsonField, JsonFilter, JsonPath, ParentFilter, Progress, ProgressMessage, StatusFilter,
    TaskDescriptor, TaskInvocation, TaskStatus,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

/// A distinct artifact key with its newest version, for `GET
/// /api/v1/artifacts`.
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
        self.sort
            .map(Into::into)
            .unwrap_or(VersionSort::DownloadedAt)
    }
}

/// Lifecycle state of a task invocation.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatusResponse {
    /// Recorded but not yet started.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Succeeded,
    /// Finished with an error.
    Failed,
    /// Stopped on request.
    Cancelled,
    /// Still running when the process stopped.
    Interrupted,
}

impl From<TaskStatus> for TaskStatusResponse {
    fn from(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Pending => Self::Pending,
            TaskStatus::Running => Self::Running,
            TaskStatus::Succeeded => Self::Succeeded,
            TaskStatus::Failed => Self::Failed,
            TaskStatus::Cancelled => Self::Cancelled,
            TaskStatus::Interrupted => Self::Interrupted,
        }
    }
}

/// A localizable progress message: a translation key and its arguments.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProgressMessageResponse {
    /// Translation key for the current step.
    pub key: String,
    /// Arguments to interpolate into the resolved message.
    pub args: HashMap<String, String>,
}

impl From<ProgressMessage> for ProgressMessageResponse {
    fn from(message: ProgressMessage) -> Self {
        Self {
            key: message.key,
            args: message.args.into_iter().collect(),
        }
    }
}

/// Live progress of a running task.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProgressResponse {
    /// A localizable description of the current step.
    pub message: Option<ProgressMessageResponse>,
    /// Units of work completed so far.
    pub done: Option<u64>,
    /// Total units of work expected, if known.
    pub total: Option<u64>,
}

impl From<Progress> for ProgressResponse {
    fn from(progress: Progress) -> Self {
        Self {
            message: progress.message.map(ProgressMessageResponse::from),
            done: progress.done,
            total: progress.total,
        }
    }
}

/// One task invocation, returned by the task endpoints.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskResponse {
    /// Invocation id.
    pub id: i64,
    /// The kind of task.
    pub kind: String,
    /// The parent invocation, if this task was spawned by another.
    pub parent_id: Option<i64>,
    /// Lifecycle state.
    pub status: TaskStatusResponse,
    /// The input the task was created with.
    pub input: Value,
    /// The output, once the task succeeds.
    pub output: Option<Value>,
    /// Failure detail, if any.
    pub error: Option<String>,
    /// When the invocation was recorded.
    pub created_at: Timestamp,
    /// When it started running, if it did.
    pub started_at: Option<Timestamp>,
    /// When it finished, if it did.
    pub finished_at: Option<Timestamp>,
    /// Live progress, present only while running.
    pub progress: Option<ProgressResponse>,
}

impl TaskResponse {
    /// Builds a response, attaching live `progress` while the task is running.
    pub(crate) fn new(task: TaskInvocation, progress: Option<Progress>) -> Self {
        let running = matches!(task.status, TaskStatus::Pending | TaskStatus::Running);
        Self {
            id: task.id,
            kind: task.kind,
            parent_id: task.parent_id,
            status: task.status.into(),
            input: parse_json(&task.input),
            output: task.output.as_deref().map(parse_json),
            error: task.error,
            created_at: task.created_at,
            started_at: task.started_at,
            finished_at: task.finished_at,
            progress: running
                .then_some(progress)
                .flatten()
                .map(ProgressResponse::from),
        }
    }
}

/// A registered task kind, with its capabilities and JSON Schemas.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskDefinitionResponse {
    /// The kind string.
    pub kind: String,
    /// Whether the kind can be cancelled.
    pub cancellable: bool,
    /// Whether the kind is safe to interrupt across a restart.
    pub resumable: bool,
    /// JSON Schema of the kind's input.
    pub input_schema: Value,
    /// JSON Schema of the kind's output.
    pub output_schema: Value,
}

impl From<TaskDescriptor> for TaskDefinitionResponse {
    fn from(descriptor: TaskDescriptor) -> Self {
        Self {
            kind: descriptor.kind.to_owned(),
            cancellable: descriptor.capabilities.cancellable,
            resumable: descriptor.capabilities.resumable,
            input_schema: serde_json::to_value(&descriptor.input_schema).unwrap_or(Value::Null),
            output_schema: serde_json::to_value(&descriptor.output_schema).unwrap_or(Value::Null),
        }
    }
}

/// Body for `POST /api/v1/tasks`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTaskRequest {
    /// The kind of task to create.
    pub kind: String,
    /// The task input, matching the kind's input schema.
    pub input: Value,
}

/// Filter for task status, including the `active` and `finished` groups.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatusParam {
    /// Not yet finished (pending or running).
    Active,
    /// Reached a terminal state.
    Finished,
    /// Recorded but not yet started.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Succeeded,
    /// Finished with an error.
    Failed,
    /// Stopped on request.
    Cancelled,
    /// Still running when the process stopped.
    Interrupted,
}

impl From<TaskStatusParam> for StatusFilter {
    fn from(param: TaskStatusParam) -> Self {
        match param {
            TaskStatusParam::Active => Self::Active,
            TaskStatusParam::Finished => Self::Finished,
            TaskStatusParam::Pending => Self::Exact(TaskStatus::Pending),
            TaskStatusParam::Running => Self::Exact(TaskStatus::Running),
            TaskStatusParam::Succeeded => Self::Exact(TaskStatus::Succeeded),
            TaskStatusParam::Failed => Self::Exact(TaskStatus::Failed),
            TaskStatusParam::Cancelled => Self::Exact(TaskStatus::Cancelled),
            TaskStatusParam::Interrupted => Self::Exact(TaskStatus::Interrupted),
        }
    }
}

/// Query params for `GET /api/v1/tasks`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct TaskListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction.
    pub order: Option<OrderParam>,
    /// Only tasks in this state, or the `active`/`finished` groups.
    pub status: Option<TaskStatusParam>,
    /// Only tasks of this kind.
    pub kind: Option<String>,
    /// Only children of this task.
    pub parent: Option<i64>,
    /// Only top-level tasks (no parent). Ignored when `parent` is set.
    pub root: Option<bool>,
    /// Only tasks whose input JSON has `input_value` at this path, for example
    /// `key` or `source.reference`. Requires `input_value`.
    pub input_path: Option<String>,
    /// The value the field at `input_path` must equal.
    pub input_value: Option<String>,
    /// Only tasks whose output JSON has `output_value` at this path. Requires
    /// `output_value`.
    pub output_path: Option<String>,
    /// The value the field at `output_path` must equal.
    pub output_value: Option<String>,
}

impl TaskListParams {
    pub(crate) fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }

    pub(crate) fn parent_filter(&self) -> Option<ParentFilter> {
        match (self.parent, self.root) {
            (Some(id), _) => Some(ParentFilter::Of(id)),
            (None, Some(true)) => Some(ParentFilter::Root),
            _ => None,
        }
    }

    /// Builds the input/output JSON filters. Returns `Err` if a path is given
    /// without its value (or the reverse) or a path is not a simple JSON path.
    pub(crate) fn json_filters(&self) -> Result<Vec<JsonFilter<'_>>, InvalidFilter> {
        let mut filters = Vec::new();
        let pairs = [
            (JsonField::Input, &self.input_path, &self.input_value),
            (JsonField::Output, &self.output_path, &self.output_value),
        ];
        for (field, path, value) in pairs {
            match (path.as_deref(), value.as_deref()) {
                (Some(path), Some(value)) => {
                    let path = JsonPath::new(path).map_err(|_| InvalidFilter)?;
                    filters.push(JsonFilter { field, path, value });
                }
                (None, None) => {}
                _ => return Err(InvalidFilter),
            }
        }
        Ok(filters)
    }
}

/// A task list request carried a malformed JSON filter.
pub(crate) struct InvalidFilter;

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

/// Maps a storage page of keys into the response envelope.
pub(crate) fn artifact_page(page: StoragePage<ArtifactKey>) -> Page<ArtifactSummaryResponse> {
    Page::from_storage(page, ArtifactSummaryResponse::from)
}

/// Maps a storage page of versions into the response envelope.
pub(crate) fn version_page(page: StoragePage<Artifact>) -> Page<ArtifactVersionResponse> {
    Page::from_storage(page, ArtifactVersionResponse::from)
}

/// Maps a storage page of tasks into the response envelope, attaching live
/// progress to running tasks from `live` (keyed by task id).
pub(crate) fn task_page(
    page: StoragePage<TaskInvocation>,
    live: &HashMap<i64, Progress>,
) -> Page<TaskResponse> {
    Page::from_storage(page, |task| {
        let progress = live.get(&task.id).cloned();
        TaskResponse::new(task, progress)
    })
}
