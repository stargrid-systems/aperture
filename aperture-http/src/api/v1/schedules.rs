use aperture_storage::DbId;
use aperture_tasks::NewSchedule;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use jiff::Timestamp;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::dto::{
    CreateScheduleRequest, Page, ScheduleListParams, ScheduleResponse, UpdateScheduleRequest,
    schedule_page,
};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_schedules, create_schedule))
        .routes(routes!(get_schedule, update_schedule, delete_schedule))
}

/// Lists every periodic schedule.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_SCHEDULES,
    params(ScheduleListParams),
    responses((status = 200, description = "Schedules", body = Page<ScheduleResponse>)),
)]
async fn list_schedules(
    State(state): State<AppState>,
    Query(params): Query<ScheduleListParams>,
) -> Result<Json<Page<ScheduleResponse>>, ApiError> {
    let page = state.scheduler().list(&params.to_query()).await?;
    Ok(Json(schedule_page(page)))
}

/// Creates a new periodic schedule. The first spawn fires at `next_run_at`,
/// which defaults to now.
#[utoipa::path(
    post,
    path = "",
    operation_id = operation_ids::CREATE_SCHEDULE,
    request_body = CreateScheduleRequest,
    responses(
        (status = 201, description = "Schedule created", body = ScheduleResponse),
        (status = 400, description = "Invalid input"),
    ),
)]
async fn create_schedule(
    State(state): State<AppState>,
    Json(request): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), ApiError> {
    if request.interval_ms <= 0 {
        return Err(ApiError::BAD_REQUEST);
    }
    let input = serde_json::to_string(&request.input).map_err(|_| ApiError::BAD_REQUEST)?;
    let now = Timestamp::now();
    let schedule = state
        .scheduler()
        .create(NewSchedule {
            kind: request.kind,
            input,
            interval_ms: request.interval_ms,
            next_run_at: now,
            created_at: now,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(schedule.into())))
}

/// Returns one schedule.
#[utoipa::path(
    get,
    path = "/{id}",
    operation_id = operation_ids::GET_SCHEDULE,
    params(("id" = DbId, Path, description = "Schedule id")),
    responses(
        (status = 200, description = "Schedule", body = ScheduleResponse),
        (status = 404, description = "Unknown schedule"),
    ),
)]
async fn get_schedule(
    State(state): State<AppState>,
    Path(id): Path<DbId>,
) -> Result<Json<ScheduleResponse>, ApiError> {
    let schedule = state
        .scheduler()
        .get(id)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(schedule.into()))
}

/// Updates a schedule's interval and/or enabled flag.
#[utoipa::path(
    patch,
    path = "/{id}",
    operation_id = operation_ids::UPDATE_SCHEDULE,
    params(("id" = DbId, Path, description = "Schedule id")),
    request_body = UpdateScheduleRequest,
    responses(
        (status = 200, description = "Schedule updated", body = ScheduleResponse),
        (status = 404, description = "Unknown schedule"),
    ),
)]
async fn update_schedule(
    State(state): State<AppState>,
    Path(id): Path<DbId>,
    Json(request): Json<UpdateScheduleRequest>,
) -> Result<Json<ScheduleResponse>, ApiError> {
    if let Some(interval_ms) = request.interval_ms
        && interval_ms <= 0
    {
        return Err(ApiError::BAD_REQUEST);
    }
    let schedule = state
        .scheduler()
        .update(id, request.to_patch())
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(schedule.into()))
}

/// Deletes a schedule.
#[utoipa::path(
    delete,
    path = "/{id}",
    operation_id = operation_ids::DELETE_SCHEDULE,
    params(("id" = DbId, Path, description = "Schedule id")),
    responses(
        (status = 204, description = "Schedule deleted"),
        (status = 404, description = "Unknown schedule"),
    ),
)]
async fn delete_schedule(
    State(state): State<AppState>,
    Path(id): Path<DbId>,
) -> Result<StatusCode, ApiError> {
    let removed = state.scheduler().delete(id).await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NOT_FOUND)
    }
}
