use aperture_auth::authz::{self, Permission};
use aperture_storage::TaskScheduleId;
use aperture_tasks::NewTaskSchedule;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use jiff::Timestamp;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::auth::Require;
use crate::dto::{
    CreateTaskScheduleRequest, Page, TaskScheduleListParams, TaskScheduleResponse,
    UpdateTaskScheduleRequest,
};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_task_schedules, create_task_schedule))
        .routes(routes!(
            get_task_schedule,
            update_task_schedule,
            delete_task_schedule
        ))
}

/// Lists every periodic task schedule.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_TASK_SCHEDULES,
    extensions(("x-required-permission" = json!(authz::task_schedule::Read::PERMISSION))),
    params(TaskScheduleListParams),
    responses((status = 200, description = "Task schedules", body = Page<TaskScheduleResponse>)),
)]
async fn list_task_schedules(
    Require(_permit): Require<authz::task_schedule::Read>,
    State(state): State<AppState>,
    Query(params): Query<TaskScheduleListParams>,
) -> Result<Json<Page<TaskScheduleResponse>>, ApiError> {
    let repo = state.storage().task_schedules()?;
    let page = repo.list(&params.to_query()).await?;
    Ok(Json(TaskScheduleResponse::page(page)))
}

/// Creates a new periodic task schedule.
#[utoipa::path(
    post,
    path = "",
    operation_id = operation_ids::CREATE_TASK_SCHEDULE,
    extensions(("x-required-permission" = json!(authz::task_schedule::Create::PERMISSION))),
    request_body = CreateTaskScheduleRequest,
    responses(
        (status = 201, description = "Task schedule created", body = TaskScheduleResponse),
        (status = 422, description = "Invalid input"),
    ),
)]
async fn create_task_schedule(
    Require(_permit): Require<authz::task_schedule::Create>,
    State(state): State<AppState>,
    Json(request): Json<CreateTaskScheduleRequest>,
) -> Result<(StatusCode, Json<TaskScheduleResponse>), ApiError> {
    let now = Timestamp::now();
    let repo = state.storage().task_schedules()?;
    let id = repo
        .create(&NewTaskSchedule {
            key: request.key,
            input: request.input,
            interval: request.interval,
            next_run_at: now,
            created_at: now,
        })
        .await?;
    let schedule = repo.get(id).await?.ok_or(ApiError::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(schedule.into())))
}

/// Returns one task schedule.
#[utoipa::path(
    get,
    path = "/{id}",
    operation_id = operation_ids::GET_TASK_SCHEDULE,
    extensions(("x-required-permission" = json!(authz::task_schedule::Read::PERMISSION))),
    params(("id" = TaskScheduleId, Path, description = "Task schedule id")),
    responses(
        (status = 200, description = "Task schedule", body = TaskScheduleResponse),
        (status = 404, description = "Unknown task schedule"),
    ),
)]
async fn get_task_schedule(
    Require(_permit): Require<authz::task_schedule::Read>,
    State(state): State<AppState>,
    Path(id): Path<TaskScheduleId>,
) -> Result<Json<TaskScheduleResponse>, ApiError> {
    let schedule = state
        .storage()
        .task_schedules()?
        .get(id)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(schedule.into()))
}

/// Updates a task schedule's interval and/or enabled flag.
#[utoipa::path(
    patch,
    path = "/{id}",
    operation_id = operation_ids::UPDATE_TASK_SCHEDULE,
    extensions(("x-required-permission" = json!(authz::task_schedule::Update::PERMISSION))),
    params(("id" = TaskScheduleId, Path, description = "Task schedule id")),
    request_body = UpdateTaskScheduleRequest,
    responses(
        (status = 200, description = "Task schedule updated", body = TaskScheduleResponse),
        (status = 404, description = "Unknown task schedule"),
    ),
)]
async fn update_task_schedule(
    Require(_permit): Require<authz::task_schedule::Update>,
    State(state): State<AppState>,
    Path(id): Path<TaskScheduleId>,
    Json(request): Json<UpdateTaskScheduleRequest>,
) -> Result<Json<TaskScheduleResponse>, ApiError> {
    let schedule = state
        .storage()
        .task_schedules()?
        .update(id, &request.into())
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(schedule.into()))
}

/// Deletes a task schedule.
#[utoipa::path(
    delete,
    path = "/{id}",
    operation_id = operation_ids::DELETE_TASK_SCHEDULE,
    extensions(("x-required-permission" = json!(authz::task_schedule::Delete::PERMISSION))),
    params(("id" = TaskScheduleId, Path, description = "Task schedule id")),
    responses(
        (status = 204, description = "Task schedule deleted"),
        (status = 404, description = "Unknown task schedule"),
    ),
)]
async fn delete_task_schedule(
    Require(_permit): Require<authz::task_schedule::Delete>,
    State(state): State<AppState>,
    Path(id): Path<TaskScheduleId>,
) -> Result<StatusCode, ApiError> {
    let removed = state.storage().task_schedules()?.delete(id).await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NOT_FOUND)
    }
}
