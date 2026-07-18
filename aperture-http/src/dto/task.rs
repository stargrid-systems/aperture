//! DTOs for the task invocation endpoints.

use std::collections::HashMap;

use aperture_artifacts::{ListQuery, Page as StoragePage};
use aperture_storage::DbId;
use aperture_tasks::{
    JsonField, JsonFilter, JsonPath, ParentFilter, Progress, ProgressMessage, StatusFilter,
    TaskDescriptor, TaskInvocation, TaskStatus,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::dto::{OrderParam, Page};

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
    pub id: DbId,
    /// The kind of task.
    pub kind: String,
    /// The parent invocation, if this task was spawned by another.
    pub parent_id: Option<DbId>,
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
    pub fn new(task: TaskInvocation, progress: Option<Progress>) -> Self {
        let running = matches!(task.status, TaskStatus::Pending | TaskStatus::Running);
        Self {
            id: task.id,
            kind: task.kind,
            parent_id: task.parent_id,
            status: task.status.into(),
            input: task.input,
            output: task.output,
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
    pub parent: Option<DbId>,
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
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }

    pub fn parent_filter(&self) -> Option<ParentFilter> {
        match (self.parent, self.root) {
            (Some(id), _) => Some(ParentFilter::Of(id)),
            (None, Some(true)) => Some(ParentFilter::Root),
            _ => None,
        }
    }

    /// Builds the input/output JSON filters. Returns `Err` if a path is given
    /// without its value (or the reverse) or a path is not a simple JSON path.
    pub fn json_filters(&self) -> Result<Vec<JsonFilter<'_>>, InvalidFilter> {
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
pub struct InvalidFilter;

/// Maps a storage page of tasks into the response envelope, attaching live
/// progress to running tasks from `live` (keyed by task id).
pub fn task_page(
    page: StoragePage<TaskInvocation>,
    live: &HashMap<DbId, Progress>,
) -> Page<TaskResponse> {
    Page::from_storage(page, |task| {
        let progress = live.get(&task.id).cloned();
        TaskResponse::new(task, progress)
    })
}
