use aperture_artifacts::{EventFilter, ListQuery, SpanFilter};
use axum::Json;
use axum::extract::{Path, Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::{
    BootResponse, LogEventResponse, LogListParams, LogSpanDetailResponse, LogSpanListParams,
    LogSpanResponse, LogTargetListParams, Page, boots_response, event_page, span_page,
};
use crate::error::ApiError;

use super::operation_ids;

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
    let fields = parse_field_filter(params.fields.as_deref())?;
    let query = params.to_query();
    let filter = EventFilter {
        min_level: params.min_level.map(Into::into),
        target: params.target,
        query: params.q,
        span_id: params.span_id,
        since: params.since,
        until: params.until,
        fields,
    };
    let logs = state.logs()?;
    let page = logs.list_events(&filter, &query).await?;
    Ok(Json(event_page(page)))
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
    let logs = state.logs()?;
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
    let logs = state.logs()?;
    let boots = logs.list_boots().await?;
    Ok(Json(boots_response(boots, state.boot_id())))
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
    let parent = match (params.parent_id, params.parent_null) {
        (Some(id), _) => aperture_artifacts::ParentFilter::ChildrenOf(id),
        (None, Some(true)) => aperture_artifacts::ParentFilter::RootOnly,
        (None, _) => aperture_artifacts::ParentFilter::default(),
    };
    let filter = SpanFilter {
        min_level: params.min_level.map(Into::into),
        target: params.target,
        since: params.since,
        until: params.until,
        parent,
    };
    let logs = state.logs()?;
    let page = logs.list_spans(&filter, &query).await?;
    Ok(Json(span_page(page)))
}

/// Returns a single span with its events.
#[utoipa::path(
    get,
    path = "/spans/{id}",
    operation_id = operation_ids::GET_SPAN,
    params(("id" = i64, Path, description = "Span id")),
    responses(
        (status = 200, description = "Span with events", body = LogSpanDetailResponse),
        (status = 404, description = "Unknown span"),
    ),
)]
async fn get_span(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<LogSpanDetailResponse>, ApiError> {
    let logs = state.logs()?;
    let span = logs.get_span(id).await?;
    let span = span.ok_or(ApiError::NOT_FOUND)?;
    let events = logs.events_for_span(id).await?;
    let events = events.into_iter().map(Into::into).collect();
    Ok(Json(LogSpanDetailResponse {
        span: span.into(),
        events,
    }))
}

/// Parses a JSON object field filter string like `{"key":"value"}` into a list
/// of key-value pairs.
fn parse_field_filter(json: Option<&str>) -> Result<Vec<(String, String)>, ApiError> {
    let Some(json) = json else {
        return Ok(Vec::new());
    };
    let obj: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|_| ApiError::BAD_REQUEST)?;
    let mut pairs = Vec::new();
    for (key, value) in obj {
        if let serde_json::Value::String(value) = value {
            pairs.push((key, value));
        } else {
            return Err(ApiError::BAD_REQUEST);
        }
    }
    Ok(pairs)
}

impl LogListParams {
    fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}

impl LogSpanListParams {
    fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}
