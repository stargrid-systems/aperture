use aperture_auth::authz::{self, Permission};
use aperture_storage::EventId;
use axum::Json;
use axum::extract::{Path, Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::auth::Require;
use crate::dto::{EventListParams, EventResponse, Page};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_events))
        .routes(routes!(get_event))
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
