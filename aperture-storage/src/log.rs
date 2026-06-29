//! Structured log storage: tracing spans and events persisted for querying.

use std::collections::HashMap;

use jiff::Timestamp;
use turso::{Connection, Row, Value, params_from_iter};

use crate::error::{Result, StorageError, database};
use crate::macros::sql;
use crate::page::{CursorValue, Filters, Keyset, ListQuery, Order, Page, Paginator};

/// Severity level of a tracing event or span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            other => Err(StorageError::Decode(format!(
                "unknown log level {other:?}"
            ))),
        }
    }

    /// Numeric severity rank. Higher is more severe. Used for `min_level`
    /// filtering via a CASE expression since SQLite has no native enum ordering.
    pub(crate) fn rank(self) -> i64 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
        }
    }
}

/// SQL CASE expression that maps the `level` text column to its numeric rank.
/// Used inside `Filters::raw` for `min_level` filtering.
const LEVEL_RANK_SQL: &str = "(CASE level \
    WHEN 'trace' THEN 0 WHEN 'debug' THEN 1 WHEN 'info' THEN 2 \
    WHEN 'warn' THEN 3 WHEN 'error' THEN 4 ELSE -1 END)";

/// A persisted tracing span.
#[derive(Debug, Clone)]
pub struct Span {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub level: Level,
    pub target: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub fields: Option<String>,
}

/// A persisted tracing event (log record).
#[derive(Debug, Clone)]
pub struct Event {
    pub id: i64,
    pub span_id: Option<i64>,
    pub level: Level,
    pub target: String,
    pub message: Option<String>,
    pub timestamp: Timestamp,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub fields: Option<String>,
}

/// Filters for log event queries.
pub struct EventFilter {
    pub min_level: Option<Level>,
    pub target: Option<String>,
    pub query: Option<String>,
    pub span_id: Option<i64>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub fields: Vec<(String, String)>,
}

/// Filters for span queries.
pub struct SpanFilter {
    pub min_level: Option<Level>,
    pub target: Option<String>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
}

/// Columns selected for an [`Event`], in [`row_to_event`] order.
const EVENT_COLUMNS: &str =
    "id, span_id, level, target, message, timestamp, file, line, fields";

/// Columns selected for a [`Span`], in [`row_to_span`] order.
const SPAN_COLUMNS: &str =
    "id, parent_id, name, level, target, file, line, started_at, ended_at, fields";

/// Repository over the structured log tables.
#[derive(Clone)]
pub struct LogRepository {
    connection: Connection,
}

impl LogRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Inserts a span and returns its assigned id.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_span(
        &self,
        parent_id: Option<i64>,
        name: &str,
        level: Level,
        target: &str,
        file: Option<&str>,
        line: Option<i64>,
        started_at: Timestamp,
        fields: Option<&str>,
    ) -> Result<i64> {
        let params = params_from_iter([
            int_or_null(parent_id),
            Value::Text(name.to_owned()),
            Value::Text(level.as_db().to_owned()),
            Value::Text(target.to_owned()),
            text_ref_or_null(file),
            int_or_null(line),
            Value::Integer(started_at.as_millisecond()),
            text_ref_or_null(fields),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO spans
                    (parent_id, name, level, target, file, line, started_at, fields)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ),
                params,
            )
            .await
            .map_err(database)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Records the end time of a span.
    pub async fn close_span(&self, id: i64, ended_at: Timestamp) -> Result<()> {
        self.connection
            .execute(
                sql!(UPDATE spans SET ended_at = ?1 WHERE id = ?2),
                params_from_iter([
                    Value::Integer(ended_at.as_millisecond()),
                    Value::Integer(id),
                ]),
            )
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Inserts a log event and returns its assigned id.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_event(
        &self,
        span_id: Option<i64>,
        level: Level,
        target: &str,
        message: Option<&str>,
        timestamp: Timestamp,
        file: Option<&str>,
        line: Option<i64>,
        fields: Option<&str>,
    ) -> Result<i64> {
        let params = params_from_iter([
            int_or_null(span_id),
            Value::Text(level.as_db().to_owned()),
            Value::Text(target.to_owned()),
            text_ref_or_null(message),
            Value::Integer(timestamp.as_millisecond()),
            text_ref_or_null(file),
            int_or_null(line),
            text_ref_or_null(fields),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO events
                    (span_id, level, target, message, timestamp, file, line, fields)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ),
                params,
            )
            .await
            .map_err(database)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Lists log events matching the given filters, ordered by timestamp
    /// descending by default.
    pub async fn list_events(
        &self,
        filter: &EventFilter,
        query: &ListQuery,
    ) -> Result<Page<Event>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::with_id("timestamp", paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.raw(&format!("{LEVEL_RANK_SQL} >= {}", min_level.rank()));
        }

        filters.prefix("target", filter.target.as_deref());
        filters.eq_int("span_id", filter.span_id);
        filters.gte_int("timestamp", filter.since.map(|ts| ts.as_millisecond()));
        filters.lte_int("timestamp", filter.until.map(|ts| ts.as_millisecond()));

        for (key, value) in &filter.fields {
            filters.json_eq(key, Some(value));
        }

        if let Some(q) = &filter.query {
            filters.param(Value::Text(q.clone()), |idx| {
                format!("message MATCH ?{idx}")
            });
        }

        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM events {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = EVENT_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(database)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            items.push(row_to_event(&row)?);
        }
        Ok(paginator.finish(items, |event| {
            (CursorValue::Int(event.timestamp.as_millisecond()), event.id)
        }))
    }

    /// Lists distinct event targets, optionally filtered by prefix.
    pub async fn list_targets(&self, q: Option<&str>) -> Result<Vec<String>> {
        let sql = match q {
            Some(_) => "SELECT DISTINCT target FROM events \
                        WHERE target LIKE ?1 ESCAPE '\\' ORDER BY target",
            None => "SELECT DISTINCT target FROM events ORDER BY target",
        };
        let params: Vec<Value> = match q {
            Some(prefix) => vec![Value::Text(format!("{}%", escape_like(prefix)))],
            None => vec![],
        };
        let mut rows = self
            .connection
            .query(sql, params_from_iter(params))
            .await
            .map_err(database)?;
        let mut targets = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            targets.push(req_text(&row, 0)?);
        }
        Ok(targets)
    }

    /// Lists spans matching the given filters, ordered by started_at descending
    /// by default.
    pub async fn list_spans(
        &self,
        filter: &SpanFilter,
        query: &ListQuery,
    ) -> Result<Page<Span>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::with_id("started_at", paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.raw(&format!("{LEVEL_RANK_SQL} >= {}", min_level.rank()));
        }

        filters.prefix("target", filter.target.as_deref());
        filters.gte_int("started_at", filter.since.map(|ts| ts.as_millisecond()));
        filters.lte_int("started_at", filter.until.map(|ts| ts.as_millisecond()));
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM spans {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = SPAN_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(database)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            items.push(row_to_span(&row)?);
        }
        Ok(paginator.finish(items, |span| {
            (CursorValue::Int(span.started_at.as_millisecond()), span.id)
        }))
    }

    /// Returns a single span by id, if it exists.
    pub async fn get_span(&self, id: i64) -> Result<Option<Span>> {
        let sql = format!(
            sql!(SELECT {cols} FROM spans WHERE id = ?1),
            cols = SPAN_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(id)]))
            .await
            .map_err(database)?;
        match rows.next().await.map_err(database)? {
            Some(row) => Ok(Some(row_to_span(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns all events belonging to `span_id`, ordered by timestamp.
    pub async fn events_for_span(&self, span_id: i64) -> Result<Vec<Event>> {
        let sql = format!(
            sql!(SELECT {cols} FROM events WHERE span_id = ?1 ORDER BY timestamp),
            cols = EVENT_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(span_id)]))
            .await
            .map_err(database)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            items.push(row_to_event(&row)?);
        }
        Ok(items)
    }

    /// Deletes events and finished spans older than `before`. Returns the number
    /// of deleted events. FTS entries are cleaned up by triggers.
    pub async fn prune_before(&self, before: Timestamp) -> Result<u64> {
        let millis = before.as_millisecond();
        let event_count = self
            .connection
            .execute(
                sql!(DELETE FROM events WHERE timestamp < ?1),
                params_from_iter([Value::Integer(millis)]),
            )
            .await
            .map_err(database)?;
        self.connection
            .execute(
                sql!(DELETE FROM spans WHERE ended_at IS NOT NULL AND ended_at < ?1),
                params_from_iter([Value::Integer(millis)]),
            )
            .await
            .map_err(database)?;
        Ok(event_count)
    }

    /// Inserts a synthetic event recording that log records were dropped.
    pub async fn record_dropped(&self, count: u64, timestamp: Timestamp) -> Result<()> {
        let fields = serde_json::to_string(&HashMap::from([("dropped", count)]))
            .map_err(|err| StorageError::Decode(format!("failed to serialize dropped fields: {err}")))?;
        self.insert_event(
            None,
            Level::Warn,
            "aperture::log",
            Some(&format!("dropped {count} log records due to full buffer")),
            timestamp,
            None,
            None,
            Some(&fields),
        )
        .await?;
        Ok(())
    }
}

fn text_ref_or_null(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::Text(text.to_owned()),
        None => Value::Null,
    }
}

fn int_or_null(value: Option<i64>) -> Value {
    match value {
        Some(int) => Value::Integer(int),
        None => Value::Null,
    }
}

fn req_text(row: &Row, idx: usize) -> Result<String> {
    match row.get_value(idx).map_err(database)? {
        Value::Text(text) => Ok(text),
        other => Err(StorageError::Decode(format!(
            "expected text at column {idx}, found {other:?}"
        ))),
    }
}

fn opt_text(row: &Row, idx: usize) -> Result<Option<String>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text)),
        other => Err(StorageError::Decode(format!(
            "expected text or null at column {idx}, found {other:?}"
        ))),
    }
}

fn req_int(row: &Row, idx: usize) -> Result<i64> {
    match row.get_value(idx).map_err(database)? {
        Value::Integer(int) => Ok(int),
        other => Err(StorageError::Decode(format!(
            "expected integer at column {idx}, found {other:?}"
        ))),
    }
}

fn opt_int(row: &Row, idx: usize) -> Result<Option<i64>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Integer(int) => Ok(Some(int)),
        other => Err(StorageError::Decode(format!(
            "expected integer or null at column {idx}, found {other:?}"
        ))),
    }
}

fn req_ts(row: &Row, idx: usize) -> Result<Timestamp> {
    ts_from_millis(req_int(row, idx)?)
}

fn opt_ts(row: &Row, idx: usize) -> Result<Option<Timestamp>> {
    match opt_int(row, idx)? {
        Some(millis) => Ok(Some(ts_from_millis(millis)?)),
        None => Ok(None),
    }
}

fn ts_from_millis(millis: i64) -> Result<Timestamp> {
    Timestamp::from_millisecond(millis)
        .map_err(|err| StorageError::Decode(format!("invalid timestamp {millis}: {err}")))
}

fn row_to_event(row: &Row) -> Result<Event> {
    Ok(Event {
        id: req_int(row, 0)?,
        span_id: opt_int(row, 1)?,
        level: Level::from_db(&req_text(row, 2)?)?,
        target: req_text(row, 3)?,
        message: opt_text(row, 4)?,
        timestamp: req_ts(row, 5)?,
        file: opt_text(row, 6)?,
        line: opt_int(row, 7)?,
        fields: opt_text(row, 8)?,
    })
}

fn row_to_span(row: &Row) -> Result<Span> {
    Ok(Span {
        id: req_int(row, 0)?,
        parent_id: opt_int(row, 1)?,
        name: req_text(row, 2)?,
        level: Level::from_db(&req_text(row, 3)?)?,
        target: req_text(row, 4)?,
        file: opt_text(row, 5)?,
        line: opt_int(row, 6)?,
        started_at: req_ts(row, 7)?,
        ended_at: opt_ts(row, 8)?,
        fields: opt_text(row, 9)?,
    })
}

/// Escapes the LIKE wildcards `%` and `_` (and the escape char itself) so a
/// user-supplied prefix matches literally. Pair with `ESCAPE '\'` in the SQL.
fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}
