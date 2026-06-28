use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::{
    CreateTaskRequest, Page, TaskDefinitionResponse, TaskListParams, TaskResponse, task_page,
};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_tasks, create_task))
        .routes(routes!(get_task))
        .routes(routes!(cancel_task))
}

pub fn definitions_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_definitions))
}

/// Lists task invocations, optionally filtered by status, kind, and parent.
/// Running tasks carry live progress.
#[utoipa::path(
    get,
    path = "",
    params(TaskListParams),
    responses((status = 200, description = "Tasks", body = Page<TaskResponse>)),
)]
async fn list_tasks(
    State(state): State<AppState>,
    Query(params): Query<TaskListParams>,
) -> Result<Json<Page<TaskResponse>>, ApiError> {
    let tasks = state.tasks();
    let page = tasks
        .list(
            params.status.map(Into::into),
            params.kind.as_deref(),
            params.parent_filter(),
            &params.to_query(),
        )
        .await?;

    let live: HashMap<i64, _> = tasks
        .active()
        .into_iter()
        .map(|task| (task.id, task.progress))
        .collect();

    Ok(Json(task_page(page, &live)))
}

/// Creates a task of the given kind and starts it. The body input is validated
/// against the kind's input schema.
#[utoipa::path(
    post,
    path = "",
    request_body = CreateTaskRequest,
    responses(
        (status = 202, description = "Task created", body = TaskResponse),
        (status = 400, description = "Unknown kind or invalid input"),
    ),
)]
async fn create_task(
    State(state): State<AppState>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    let id = state.tasks().create(&request.kind, request.input).await?;
    let task = state.tasks().get(id).await?.ok_or(ApiError::NOT_FOUND)?;
    Ok((StatusCode::ACCEPTED, Json(TaskResponse::new(task, None))))
}

/// Returns one task invocation.
#[utoipa::path(
    get,
    path = "/{id}",
    params(("id" = i64, Path, description = "Task id")),
    responses(
        (status = 200, description = "Task", body = TaskResponse),
        (status = 404, description = "Unknown task"),
    ),
)]
async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskResponse>, ApiError> {
    let task = state.tasks().get(id).await?.ok_or(ApiError::NOT_FOUND)?;
    let progress = state.tasks().progress(id);
    Ok(Json(TaskResponse::new(task, progress)))
}

/// Requests cooperative cancellation of a running task.
#[utoipa::path(
    post,
    path = "/{id}/cancel",
    params(("id" = i64, Path, description = "Task id")),
    responses(
        (status = 202, description = "Cancellation requested"),
        (status = 404, description = "Task is not running"),
        (status = 409, description = "Task kind cannot be cancelled"),
    ),
)]
async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if state.tasks().cancel(id)? {
        Ok(StatusCode::ACCEPTED)
    } else {
        Err(ApiError::CONFLICT)
    }
}

/// Lists the registered task kinds with their capabilities and JSON Schemas.
#[utoipa::path(
    get,
    path = "",
    responses((status = 200, description = "Task definitions", body = Vec<TaskDefinitionResponse>)),
)]
async fn list_definitions(State(state): State<AppState>) -> Json<Vec<TaskDefinitionResponse>> {
    let definitions = state
        .tasks()
        .registry()
        .descriptors()
        .into_iter()
        .map(TaskDefinitionResponse::from)
        .collect();
    Json(definitions)
}
