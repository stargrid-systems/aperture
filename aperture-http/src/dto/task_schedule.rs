use aperture_artifacts::{ListQuery, Page as StoragePage};
use aperture_storage::{Interval, TaskId, TaskSchedule, TaskScheduleId};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::dto::{OrderParam, Page};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskScheduleResponse {
    pub id: TaskScheduleId,
    pub kind: String,
    pub input: Value,
    pub interval: Interval,
    pub next_run_at: Timestamp,
    pub last_run_at: Option<Timestamp>,
    pub last_task_id: Option<TaskId>,
    pub enabled: bool,
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

impl TaskScheduleResponse {
    /// Maps a storage page of schedules into the response envelope.
    pub fn page(page: StoragePage<TaskSchedule>) -> Page<Self> {
        Page::from_storage(page, Self::from)
    }
}

/// Body for `POST /api/v1/task-schedules`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateTaskScheduleRequest {
    pub kind: String,
    pub input: Value,
    pub interval: Interval,
}

/// Body for `PATCH /api/v1/task-schedules/{id}`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct UpdateTaskScheduleRequest {
    pub interval: Option<Interval>,
    pub enabled: Option<bool>,
}

impl From<UpdateTaskScheduleRequest> for aperture_storage::TaskSchedulePatch {
    fn from(request: UpdateTaskScheduleRequest) -> Self {
        aperture_storage::TaskSchedulePatch {
            interval: request.interval,
            enabled: request.enabled,
        }
    }
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct TaskScheduleListParams {
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
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
