//! DTOs for the structured log endpoints.

use aperture_artifacts::{ListQuery, Page as StoragePage};
use aperture_storage::{BootInfo, Event, EventId, Level, Span, SpanId};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Map;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::dto::{JsonQueryString, OrderParam, Page, deserialize_single_or_vec_string};

/// Severity level of a log event or span.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LevelResponse {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<Level> for LevelResponse {
    fn from(level: Level) -> Self {
        match level {
            Level::Trace => Self::Trace,
            Level::Debug => Self::Debug,
            Level::Info => Self::Info,
            Level::Warn => Self::Warn,
            Level::Error => Self::Error,
        }
    }
}

impl From<LevelResponse> for Level {
    fn from(level: LevelResponse) -> Self {
        match level {
            LevelResponse::Trace => Self::Trace,
            LevelResponse::Debug => Self::Debug,
            LevelResponse::Info => Self::Info,
            LevelResponse::Warn => Self::Warn,
            LevelResponse::Error => Self::Error,
        }
    }
}

/// A single log event, returned by `GET /api/v1/logs`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LogEventResponse {
    /// Event id.
    pub id: EventId,
    /// Span this event belongs to, if any.
    pub span_id: Option<SpanId>,
    /// Severity level.
    pub level: LevelResponse,
    /// Module path that emitted the event.
    pub target: String,
    /// Human-readable message, if any.
    pub message: Option<String>,
    /// When the event was emitted.
    pub timestamp: Timestamp,
    /// Source file, if available.
    pub file: Option<String>,
    /// Source line, if available.
    pub line: Option<u32>,
    /// Boot session this event belongs to, if known.
    pub boot_id: Uuid,
    /// Structured fields as a JSON object, if any.
    pub fields: Map<String, serde_json::Value>,
}

impl From<Event> for LogEventResponse {
    fn from(event: Event) -> Self {
        Self {
            id: event.id,
            span_id: event.span_id,
            level: event.level.into(),
            target: event.target,
            message: event.message,
            timestamp: event.timestamp,
            file: event.file,
            line: event.line,
            boot_id: event.boot_id,
            fields: event.fields,
        }
    }
}

impl LogEventResponse {
    /// Maps a storage page of events into the response envelope.
    pub fn page(page: StoragePage<Event>) -> Page<Self> {
        Page::from_storage(page, Self::from)
    }
}

/// A tracing span, returned by `GET /api/v1/logs/spans`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LogSpanResponse {
    /// Span id.
    pub id: SpanId,
    /// Parent span id, if any.
    pub parent_id: Option<SpanId>,
    /// Span name.
    pub name: String,
    /// Severity level.
    pub level: LevelResponse,
    /// Module path that created the span.
    pub target: String,
    /// Source file, if available.
    pub file: Option<String>,
    /// Source line, if available.
    pub line: Option<u32>,
    /// When the span started.
    pub started_at: Timestamp,
    /// When the span ended, if it did.
    pub ended_at: Option<Timestamp>,
    /// Span fields as a JSON object, if any.
    pub fields: Map<String, serde_json::Value>,
}

impl From<Span> for LogSpanResponse {
    fn from(span: Span) -> Self {
        Self {
            id: span.id,
            parent_id: span.parent_id,
            name: span.name,
            level: span.level.into(),
            target: span.target,
            file: span.file,
            line: span.line,
            started_at: span.started_at,
            ended_at: span.ended_at,
            fields: span.fields,
        }
    }
}

impl LogSpanResponse {
    /// Maps a storage page of spans into the response envelope.
    pub fn page(page: StoragePage<Span>) -> Page<Self> {
        Page::from_storage(page, Self::from)
    }
}

/// A span with its child events, returned by `GET /api/v1/logs/spans/{id}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LogSpanDetailResponse {
    #[serde(flatten)]
    pub span: LogSpanResponse,
    /// Events belonging to this span, ordered by timestamp.
    pub events: Vec<LogEventResponse>,
}

/// Query params for `GET /api/v1/logs`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct LogListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction. Defaults to descending (newest first).
    pub order: Option<OrderParam>,
    /// Only events at this severity or higher.
    pub min_level: Option<LevelResponse>,
    /// Only events whose target is one of these (comma-separated). Example:
    /// `aperture,aperture_http`.
    #[serde(default, deserialize_with = "deserialize_single_or_vec_string")]
    pub target: Vec<String>,
    /// Substring search across message and target.
    pub q: Option<String>,
    /// Only events belonging to this span.
    pub span_id: Option<SpanId>,
    /// Only events from this boot session.
    pub boot_id: Option<Uuid>,
    /// Only events at or after this time (RFC 3339).
    pub since: Option<Timestamp>,
    /// Only events at or before this time (RFC 3339).
    pub until: Option<Timestamp>,
    /// Structured field filter as a JSON object, e.g. `{"key":"value"}`.
    pub fields: Option<JsonQueryString>,
}

impl LogListParams {
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}

/// Query params for `GET /api/v1/logs/spans`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct LogSpanListParams {
    /// Maximum rows to return. Defaults to 50.
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    /// Cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction. Defaults to descending (newest first).
    pub order: Option<OrderParam>,
    /// Only spans at this severity or higher.
    pub min_level: Option<LevelResponse>,
    /// Only spans whose target is one of these (comma-separated). Example:
    /// `aperture,aperture_storage`.
    #[serde(default, deserialize_with = "deserialize_single_or_vec_string")]
    pub target: Vec<String>,
    /// Only spans from this boot session.
    pub boot_id: Option<Uuid>,
    /// Only spans started at or after this time (RFC 3339).
    pub since: Option<Timestamp>,
    /// Only spans started at or before this time (RFC 3339).
    pub until: Option<Timestamp>,
    /// Only direct children of this span id.
    pub parent_id: Option<SpanId>,
    /// When true, only root spans (no parent) are returned.
    pub parent_null: Option<bool>,
    /// Structured field filter as a JSON object, e.g. `{"key":"value"}`.
    pub fields: Option<JsonQueryString>,
}

impl LogSpanListParams {
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}

/// Query params for `GET /api/v1/logs/targets`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct LogTargetListParams {
    /// Only targets starting with this prefix.
    pub q: Option<String>,
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub order: Option<OrderParam>,
}

impl LogTargetListParams {
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}

/// One boot session observed in the log store, returned by
/// `GET /api/v1/logs/boots`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BootResponse {
    /// Unique boot id (UUID).
    pub boot_id: Uuid,
    /// Timestamp of the earliest event in this boot.
    pub first_seen: Timestamp,
    /// Timestamp of the latest event in this boot.
    pub last_seen: Timestamp,
    /// Number of events recorded so far in this boot.
    pub event_count: u64,
    /// True if this is the currently running gateway boot.
    pub is_current: bool,
}

impl BootResponse {
    /// Maps a list of storage [`BootInfo`] into boot responses, marking the
    /// current boot id.
    pub fn from_boots(boots: Vec<BootInfo>, current_boot_id: Uuid) -> Vec<Self> {
        boots
            .into_iter()
            .map(|b| Self {
                is_current: b.boot_id == current_boot_id,
                boot_id: b.boot_id,
                first_seen: b.first_seen,
                last_seen: b.last_seen,
                event_count: b.event_count,
            })
            .collect()
    }
}
