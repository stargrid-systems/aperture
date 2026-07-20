use aperture_storage::{DbId, EventFilter, SpanFilter, SpanParentFilter};
use axum::Json;
use axum::extract::{Path, Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::dto::{
    BootResponse, LogEventResponse, LogListParams, LogSpanDetailResponse, LogSpanListParams,
    LogSpanResponse, LogTargetListParams, Page,
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
    params(LogListParams),
    responses((status = 200, description = "Log events", body = Page<LogEventResponse>)),
)]
async fn list_logs(
    State(state): State<AppState>,
    Query(params): Query<LogListParams>,
) -> Result<Json<Page<LogEventResponse>>, ApiError> {
    let query = params.to_query();
    let fields = params.fields.map(|f| f.into_pairs()).unwrap_or_default();
    let filter = EventFilter {
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
    params(LogTargetListParams),
    responses((status = 200, description = "Target names", body = Vec<String>)),
)]
async fn list_log_targets(
    State(state): State<AppState>,
    Query(params): Query<LogTargetListParams>,
) -> Result<Json<Vec<String>>, ApiError> {
    let logs = state.storage().logs()?;
    let targets = logs.list_targets(params.q.as_deref()).await?;
    Ok(Json(targets))
}

/// Lists distinct boot sessions, newest first.
#[utoipa::path(
    get,
    path = "/boots",
    operation_id = operation_ids::LIST_LOG_BOOTS,
    responses((status = 200, description = "Boot sessions", body = Vec<BootResponse>)),
)]
async fn list_log_boots(
    State(state): State<AppState>,
) -> Result<Json<Vec<BootResponse>>, ApiError> {
    let logs = state.storage().logs()?;
    let boots = logs.list_boots().await?;
    Ok(Json(BootResponse::from_boots(boots, state.boot_id())))
}

/// Lists tracing spans with optional filtering.
#[utoipa::path(
    get,
    path = "/spans",
    operation_id = operation_ids::LIST_SPANS,
    params(LogSpanListParams),
    responses((status = 200, description = "Spans", body = Page<LogSpanResponse>)),
)]
async fn list_spans(
    State(state): State<AppState>,
    Query(params): Query<LogSpanListParams>,
) -> Result<Json<Page<LogSpanResponse>>, ApiError> {
    let query = params.to_query();
    let fields = params.fields.map(|f| f.into_pairs()).unwrap_or_default();
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
    params(("id" = DbId, Path, description = "Span id")),
    responses(
        (status = 200, description = "Span with events", body = LogSpanDetailResponse),
        (status = 404, description = "Unknown span"),
    ),
)]
async fn get_span(
    State(state): State<AppState>,
    Path(id): Path<DbId>,
) -> Result<Json<LogSpanDetailResponse>, ApiError> {
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
