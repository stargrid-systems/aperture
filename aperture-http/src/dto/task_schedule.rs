//! DTOs for the periodic task schedule endpoints.

use aperture_artifacts::{ListQuery, Page as StoragePage};
use aperture_storage::{DbId, Interval, TaskSchedule};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::dto::{OrderParam, Page};

/// One periodic task schedule, returned by the schedule endpoints.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskScheduleResponse {
    /// Schedule id.
    pub id: DbId,
    /// The kind of task to spawn, matching a registered definition.
    pub kind: String,
    /// JSON input passed to each spawned invocation.
    pub input: Value,
    /// Spawn cadence, as an ISO 8601 duration (e.g. `PT5M`).
    pub interval: Interval,
    /// When the next spawn is due.
    pub next_run_at: Timestamp,
    /// When the most recent spawn fired, if any.
    pub last_run_at: Option<Timestamp>,
    /// The id of the most recent spawned invocation, if any.
    pub last_task_id: Option<DbId>,
    /// Whether the scheduler will fire this schedule.
    pub enabled: bool,
    /// When the schedule was created.
    pub created_at: Timestamp,
}

impl From<TaskSchedule> for TaskScheduleResponse {
    fn from(schedule: TaskSchedule) -> Self {
        Self {
            id: schedule.id,
            kind: schedule.kind,
            input: schedule.input,
            interval: schedule.interval,
            next_run_at: schedule.next_run_at,
            last_run_at: schedule.last_run_at,
            last_task_id: schedule.last_task_id,
            enabled: schedule.enabled,
            created_at: schedule.created_at,
        }
    }
}

/// Body for `POST /api/v1/task-schedules`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateTaskScheduleRequest {
    /// The kind of task to spawn.
    pub kind: String,
    /// JSON input for each spawned invocation.
    pub input: Value,
    /// Spawn cadence, as an ISO 8601 duration (e.g. `PT5M`). Must be positive
    /// and use fixed units (at most hours).
    pub interval: Interval,
}

/// Body for `PATCH /api/v1/task-schedules/{id}`. All fields optional.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct UpdateTaskScheduleRequest {
    /// New spawn cadence, as an ISO 8601 duration. Must be positive.
    pub interval: Option<Interval>,
    /// Whether the scheduler should fire this schedule.
    pub enabled: Option<bool>,
}

impl UpdateTaskScheduleRequest {
    pub fn to_patch(&self) -> aperture_storage::TaskSchedulePatch {
        aperture_storage::TaskSchedulePatch {
            interval: self.interval.clone(),
            enabled: self.enabled,
        }
    }
}

/// Query params for `GET /api/v1/task-schedules`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct TaskScheduleListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction.
    pub order: Option<OrderParam>,
}

impl TaskScheduleListParams {
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}

/// Maps a storage page of schedules into the response envelope.
pub fn task_schedule_page(page: StoragePage<TaskSchedule>) -> Page<TaskScheduleResponse> {
    Page::from_storage(page, TaskScheduleResponse::from)
}
