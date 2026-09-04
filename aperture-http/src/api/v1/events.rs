use aperture_auth::authz::{self, Permission};
use aperture_storage::EventId;
use axum::Json;
use axum::extract::{Path, Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::auth::Require;
use crate::dto::{
    EventDefinitionResponse, EventDefinitionSummary, EventListParams, EventResponse, Page,
    SimpleListParams,
};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_events))
        .routes(routes!(get_event))
}

pub fn definitions_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_definitions))
        .routes(routes!(get_definition))
}

/// Lists domain events with optional filtering.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_EVENTS,
    extensions(("x-required-permission" = json!(authz::event::Read::PERMISSION))),
    params(EventListParams),
    responses((status = 200, description = "Domain events", body = Page<EventResponse>)),
)]
async fn list_events(
    Require(_permit): Require<authz::event::Read>,
    State(state): State<AppState>,
    Query(params): Query<EventListParams>,
) -> Result<Json<Page<EventResponse>>, ApiError> {
    let events = state.storage().events()?;
    let page = events.list(&params.to_filter(), &params.to_query()).await?;
    Ok(Json(EventResponse::page(page)))
}

/// Returns a single event by id.
#[utoipa::path(
    get,
    path = "/{id}",
    operation_id = operation_ids::GET_EVENT,
    extensions(("x-required-permission" = json!(authz::event::Read::PERMISSION))),
    params(("id" = EventId, Path, description = "Event id")),
    responses(
        (status = 200, description = "Event", body = EventResponse),
        (status = 404, description = "Unknown event"),
    ),
)]
async fn get_event(
    Require(_permit): Require<authz::event::Read>,
    State(state): State<AppState>,
    Path(id): Path<EventId>,
) -> Result<Json<EventResponse>, ApiError> {
    let events = state.storage().events()?;
    let event = events.get(id).await?.ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(event.into()))
}

/// Lists the registered event definitions.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_EVENT_DEFINITIONS,
    params(SimpleListParams),
    security(()),
    responses((
        status = 200,
        description = "Event definitions",
        body = Page<EventDefinitionSummary>,
    )),
)]
async fn list_definitions(
    State(state): State<AppState>,
    Query(params): Query<SimpleListParams>,
) -> Result<Json<Page<EventDefinitionSummary>>, ApiError> {
    let page = state
        .event_registry()
        .list(&params.to_query())
        .map_err(|_| ApiError::BAD_REQUEST)?;
    Ok(Json(Page::from_registry(page, |definition| {
        EventDefinitionSummary {
            key: definition.key().to_owned(),
        }
    })))
}

/// Returns one registered event definition with its full JSON Schema.
#[utoipa::path(
    get,
    path = "/{key}",
    operation_id = operation_ids::GET_EVENT_DEFINITION,
    params(("key" = String, Path, description = "Event definition key")),
    security(()),
    responses(
        (status = 200, description = "Event definition", body = EventDefinitionResponse),
        (status = 404, description = "Unknown event definition key"),
    ),
)]
async fn get_definition(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<EventDefinitionResponse>, ApiError> {
    let definition = state
        .event_registry()
        .get(&key)
        .ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(EventDefinitionResponse {
        key: definition.key().to_owned(),
        payload_schema: definition.payload_schema(),
    }))
}
