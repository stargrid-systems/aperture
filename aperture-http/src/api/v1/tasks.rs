use std::collections::HashMap;

use aperture_auth::AuthenticatedActor;
use aperture_storage::TaskId;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
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
    operation_id = operation_ids::LIST_TASKS,
    params(TaskListParams),
    responses((status = 200, description = "Tasks", body = Page<TaskResponse>)),
)]
async fn list_tasks(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<TaskListParams>,
) -> Result<Json<Page<TaskResponse>>, ApiError> {
    if !state.auth().enforce(&auth.subject, "task", "read").await? {
        return Err(ApiError::FORBIDDEN);
    }
    let tasks = state.tasks();
    let json = params.json_filters().map_err(|_| ApiError::BAD_REQUEST)?;
    let parent = params.parent_filter();
    let page = tasks
        .list(
            params.status.map(Into::into),
            params.kind.as_deref(),
            parent,
            &json,
            &params.to_query(),
        )
        .await?;

    let live: HashMap<TaskId, _> = tasks
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
    operation_id = operation_ids::CREATE_TASK,
    request_body = CreateTaskRequest,
    responses(
        (status = 202, description = "Task created", body = TaskResponse),
        (status = 400, description = "Unknown kind or invalid input"),
    ),
)]
async fn create_task(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    state
        .auth()
        .enforce(&auth.subject, "task", "create")
        .await?;
    let task = state
        .tasks()
        .create(&request.kind, request.input, auth.actor.id)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(TaskResponse::new(task, None))))
}

/// Returns one task invocation.
#[utoipa::path(
    get,
    path = "/{id}",
    operation_id = operation_ids::GET_TASK,
    params(("id" = TaskId, Path, description = "Task id")),
    responses(
        (status = 200, description = "Task", body = TaskResponse),
        (status = 404, description = "Unknown task"),
    ),
)]
async fn get_task(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(id): Path<TaskId>,
) -> Result<Json<TaskResponse>, ApiError> {
    if !state.auth().enforce(&auth.subject, "task", "read").await? {
        return Err(ApiError::FORBIDDEN);
    }
    let task = state.tasks().get(id).await?.ok_or(ApiError::NOT_FOUND)?;
    let progress = state.tasks().progress(id);
    Ok(Json(TaskResponse::new(task, progress)))
}

/// Requests cooperative cancellation of a running task.
#[utoipa::path(
    post,
    path = "/{id}/cancel",
    operation_id = operation_ids::CANCEL_TASK,
    params(("id" = TaskId, Path, description = "Task id")),
    responses(
        (status = 202, description = "Cancellation requested"),
        (status = 404, description = "Unknown task"),
        (status = 409, description = "Task kind cannot be cancelled"),
        (status = 410, description = "Task has already finished"),
    ),
)]
async fn cancel_task(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(id): Path<TaskId>,
) -> Result<StatusCode, ApiError> {
    if !state
        .auth()
        .enforce(&auth.subject, "task", "cancel")
        .await?
    {
        return Err(ApiError::FORBIDDEN);
    }
    if state.tasks().cancel(id).await? {
        Ok(StatusCode::ACCEPTED)
    } else {
        Err(ApiError::CONFLICT)
    }
}

/// Lists the registered task kinds with their capabilities and JSON Schemas.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_TASK_DEFINITIONS,
    responses((status = 200, description = "Task definitions", body = Vec<TaskDefinitionResponse>)),
)]
async fn list_definitions(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
) -> Result<Json<Vec<TaskDefinitionResponse>>, ApiError> {
    if !state
        .auth()
        .enforce(&auth.subject, "task-definition", "read")
        .await?
    {
        return Err(ApiError::FORBIDDEN);
    }
    let definitions = state
        .tasks()
        .registry()
        .descriptors()
        .into_iter()
        .map(TaskDefinitionResponse::from)
        .collect();
    Ok(Json(definitions))
}
