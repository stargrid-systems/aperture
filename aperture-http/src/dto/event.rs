//! DTOs for the event endpoints.

use aperture_artifacts::{ListQuery, Page as StoragePage};
use aperture_storage::{ActorId, Event, EventFilter, EventId};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::dto::{OrderParam, Page};

/// A domain event, returned by `GET /api/v1/events`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EventResponse {
    /// Unique event id (UUID).
    pub id: EventId,
    /// Event key, e.g. `artifact.written`.
    pub key: String,
    /// Event-specific payload.
    pub data: Value,
    /// Actor that triggered the event.
    pub actor: ActorId,
    /// When the event was emitted.
    pub timestamp: Timestamp,
}

impl From<Event> for EventResponse {
    fn from(event: Event) -> Self {
        Self {
            id: event.id,
            key: event.key,
            data: event.data,
            actor: event.actor,
            timestamp: event.timestamp,
        }
    }
}

impl EventResponse {
    /// Maps a storage page of events into the response envelope.
    pub fn page(page: StoragePage<Event>) -> Page<Self> {
        Page::from_storage(page, Self::from)
    }
}

/// Query parameters for `GET /api/v1/events`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct EventListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction. Defaults to descending (newest first).
    pub order: Option<OrderParam>,
    /// Filter by exact key, e.g. `artifact.written`.
    pub key: Option<String>,
    /// Filter by key prefix, e.g. `artifact` matches all artifact events.
    pub key_prefix: Option<String>,
    /// Only events at or after this time (RFC 3339).
    pub since: Option<Timestamp>,
    /// Only events at or before this time (RFC 3339).
    pub until: Option<Timestamp>,
}

impl EventListParams {
    /// Converts these params into a storage `ListQuery`.
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }

    /// Converts these params into a storage `EventFilter`.
    pub fn to_filter(&self) -> EventFilter {
        EventFilter {
            key: self.key.clone(),
            key_prefix: self.key_prefix.clone(),
            since: self.since,
            until: self.until,
        }
    }
}

/// A registered event definition in a listing.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EventDefinitionSummary {
    /// The key string.
    pub key: String,
}

/// One registered event definition, with its full JSON Schema.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EventDefinitionResponse {
    /// The key string.
    pub key: String,
    /// Standalone JSON Schema (draft 2020-12) of the event's payload type.
    pub payload_schema: Value,
}
