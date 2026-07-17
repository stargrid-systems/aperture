//! DTOs for the periodic schedule endpoints.

use aperture_artifacts::{ListQuery, Page as StoragePage};
use aperture_storage::{DbId, Schedule};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::dto::{OrderParam, Page};

/// One periodic schedule, returned by the schedule endpoints.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ScheduleResponse {
    /// Schedule id.
    pub id: DbId,
    /// The kind of task to spawn, matching a registered definition.
    pub kind: String,
    /// JSON input passed to each spawned invocation.
    pub input: Value,
    /// Spawn cadence in milliseconds.
    pub interval_ms: i64,
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

impl From<Schedule> for ScheduleResponse {
    fn from(schedule: Schedule) -> Self {
        let input = serde_json::from_str(&schedule.input).unwrap_or_else(|_| Value::Null);
        Self {
            id: schedule.id,
            kind: schedule.kind,
            input,
            interval_ms: schedule.interval_ms,
            next_run_at: schedule.next_run_at,
            last_run_at: schedule.last_run_at,
            last_task_id: schedule.last_task_id,
            enabled: schedule.enabled,
            created_at: schedule.created_at,
        }
    }
}

/// Body for `POST /api/v1/schedules`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateScheduleRequest {
    /// The kind of task to spawn.
    pub kind: String,
    /// JSON input for each spawned invocation.
    pub input: Value,
    /// Spawn cadence in milliseconds. Must be positive.
    pub interval_ms: i64,
}

/// Body for `PATCH /api/v1/schedules/{id}`. All fields optional.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct UpdateScheduleRequest {
    /// New spawn cadence in milliseconds.
    pub interval_ms: Option<i64>,
    /// Whether the scheduler should fire this schedule.
    pub enabled: Option<bool>,
}

impl UpdateScheduleRequest {
    pub fn to_patch(&self) -> aperture_storage::SchedulePatch {
        aperture_storage::SchedulePatch {
            interval_ms: self.interval_ms,
            enabled: self.enabled,
        }
    }
}

/// Query params for `GET /api/v1/schedules`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct ScheduleListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction.
    pub order: Option<OrderParam>,
}

impl ScheduleListParams {
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}

/// Maps a storage page of schedules into the response envelope.
pub fn schedule_page(page: StoragePage<Schedule>) -> Page<ScheduleResponse> {
    Page::from_storage(page, ScheduleResponse::from)
}
