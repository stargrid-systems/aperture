use aperture_auth::{Action, AuthenticatedActor, Object, required_permission};
use aperture_storage::{LogEventFilter, SpanFilter, SpanId, SpanParentFilter};
use axum::Json;
use axum::extract::{Path, Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::dto::{
    BootResponse, JsonQueryString, LogEventResponse, LogListParams, LogSpanDetailResponse,
    LogSpanListParams, LogSpanResponse, LogTargetListParams, Page, SimpleListParams,
};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_logs))
        .routes(routes!(list_log_targets))
        .routes(routes!(list_log_boots))
        .routes(routes!(list_spans))
        .routes(routes!(get_span))
}

/// Lists log events with optional filtering.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_LOGS,
    extensions(("x-required-permission" = json!(required_permission(Object::Log, Action::Read)))),
    params(LogListParams),
    responses((status = 200, description = "Log events", body = Page<LogEventResponse>)),
)]
async fn list_logs(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<LogListParams>,
) -> Result<Json<Page<LogEventResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Log, Action::Read)
        .await?;
    let query = params.to_query();
    let fields = params
        .fields
        .map(JsonQueryString::into_pairs)
        .unwrap_or_default();
    let filter = LogEventFilter {
        min_level: params.min_level.map(Into::into),
        target: params.target,
        query: params.q,
        span_id: params.span_id,
        boot_id: params.boot_id,
        since: params.since,
        until: params.until,
        fields,
    };
    let logs = state.storage().logs()?;
    let page = logs.list_events(&filter, &query).await?;
    Ok(Json(LogEventResponse::page(page)))
}

/// Lists distinct log targets for autocomplete.
#[utoipa::path(
    get,
    path = "/targets",
    operation_id = operation_ids::LIST_LOG_TARGETS,
    extensions(("x-required-permission" = json!(required_permission(Object::Log, Action::Read)))),
    params(LogTargetListParams),
    responses((status = 200, description = "Target names", body = Page<String>)),
)]
async fn list_log_targets(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<LogTargetListParams>,
) -> Result<Json<Page<String>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Log, Action::Read)
        .await?;
    let logs = state.storage().logs()?;
    let page = logs
        .list_targets(params.q.as_deref(), &params.to_query())
        .await?;
    Ok(Json(Page::from_storage(page, |x| x)))
}

/// Lists distinct boot sessions, newest first.
#[utoipa::path(
    get,
    path = "/boots",
    operation_id = operation_ids::LIST_LOG_BOOTS,
    extensions(("x-required-permission" = json!(required_permission(Object::Log, Action::Read)))),
    params(SimpleListParams),
    responses((status = 200, description = "Boot sessions", body = Page<BootResponse>)),
)]
async fn list_log_boots(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<SimpleListParams>,
) -> Result<Json<Page<BootResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Log, Action::Read)
        .await?;
    let logs = state.storage().logs()?;
    let page = logs.list_boots(&params.to_query()).await?;
    let current = state.boot_id();
    Ok(Json(Page::from_storage(page, |b| BootResponse {
        boot_id: b.boot_id,
        first_seen: b.first_seen,
        last_seen: b.last_seen,
        event_count: b.event_count,
        is_current: b.boot_id == current,
    })))
}

/// Lists tracing spans with optional filtering.
#[utoipa::path(
    get,
    path = "/spans",
    operation_id = operation_ids::LIST_SPANS,
    extensions(("x-required-permission" = json!(required_permission(Object::Log, Action::Read)))),
    params(LogSpanListParams),
    responses((status = 200, description = "Spans", body = Page<LogSpanResponse>)),
)]
async fn list_spans(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<LogSpanListParams>,
) -> Result<Json<Page<LogSpanResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Log, Action::Read)
        .await?;
    let query = params.to_query();
    let fields = params
        .fields
        .map(JsonQueryString::into_pairs)
        .unwrap_or_default();
    let parent = match (params.parent_id, params.parent_null) {
        (Some(id), _) => SpanParentFilter::ChildrenOf(id),
        (None, Some(true)) => SpanParentFilter::RootOnly,
        (None, _) => SpanParentFilter::default(),
    };
    let filter = SpanFilter {
        min_level: params.min_level.map(Into::into),
        target: params.target,
        boot_id: params.boot_id,
        since: params.since,
        until: params.until,
        parent,
        fields,
    };
    let logs = state.storage().logs()?;
    let page = logs.list_spans(&filter, &query).await?;
    Ok(Json(LogSpanResponse::page(page)))
}

/// Returns a single span with its events.
#[utoipa::path(
    get,
    path = "/spans/{id}",
    operation_id = operation_ids::GET_SPAN,
    extensions(("x-required-permission" = json!(required_permission(Object::Log, Action::Read)))),
    params(("id" = SpanId, Path, description = "Span id")),
    responses(
        (status = 200, description = "Span with events", body = LogSpanDetailResponse),
        (status = 404, description = "Unknown span"),
    ),
)]
async fn get_span(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(id): Path<SpanId>,
) -> Result<Json<LogSpanDetailResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::Log, Action::Read)
        .await?;
    let logs = state.storage().logs()?;
    let span = logs.get_span(id).await?;
    let span = span.ok_or(ApiError::NOT_FOUND)?;
    let events = logs.events_for_span(id).await?;
    let events = events.into_iter().map(Into::into).collect();
    Ok(Json(LogSpanDetailResponse {
        span: span.into(),
        events,
    }))
}
