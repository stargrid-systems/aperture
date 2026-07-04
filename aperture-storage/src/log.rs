//! Structured log storage: tracing spans and events persisted for querying.

use std::collections::HashMap;

use jiff::Timestamp;
use turso::{Connection, Row, Statement, Value, params_from_iter};
use uuid::Uuid;

use crate::error::{Result, StorageError, database};
use crate::macros::sql;
use crate::page::{CursorValue, Filters, Keyset, ListQuery, Order, Page, Paginator};

const SQL_INSERT_SPAN: &str = sql!(
    INSERT INTO spans
    (parent_id, name, level, target, file, line, started_at, fields)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
);

const SQL_INSERT_EVENT: &str = sql!(
    INSERT INTO events
    (span_id, level, target, message, timestamp, file, line, fields)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
);

const SQL_CLOSE_SPAN: &str = sql!(UPDATE spans SET ended_at = ?1 WHERE id = ?2);

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
            other => Err(StorageError::Decode(format!("unknown log level {other:?}"))),
        }
    }

    /// Numeric severity rank. Higher is more severe. Used for `min_level`
    /// filtering via a CASE expression since SQLite has no native enum
    /// ordering.
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
const LEVEL_RANK_SQL: &str = "(CASE level WHEN 'trace' THEN 0 WHEN 'debug' THEN 1 WHEN 'info' \
                              THEN 2 WHEN 'warn' THEN 3 WHEN 'error' THEN 4 ELSE -1 END)";

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

/// One boot session observed in the log store.
#[derive(Debug, Clone)]
pub struct BootInfo {
    pub boot_id: Uuid,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub event_count: i64,
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

/// Filter for the `parent_id` column of a span query.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ParentFilter {
    /// No parent filtering.
    #[default]
    Any,
    /// Only root spans (parent_id IS NULL).
    RootOnly,
    /// Only direct children of the given span id.
    ChildrenOf(i64),
}

/// Filters for span queries.
#[derive(Default)]
pub struct SpanFilter {
    pub min_level: Option<Level>,
    pub target: Option<String>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub parent: ParentFilter,
}

/// Plain-data description of a span to persist via [`LogWriter::insert_span`].
pub struct SpanRecord<'a> {
    pub parent_id: Option<i64>,
    pub name: &'a str,
    pub level: Level,
    pub target: &'a str,
    pub file: Option<&'a str>,
    pub line: Option<i64>,
    pub started_at: Timestamp,
    pub fields: Option<&'a str>,
}

/// Plain-data description of an event to persist via
/// [`LogWriter::insert_event`].
pub struct EventRecord<'a> {
    pub span_id: Option<i64>,
    pub level: Level,
    pub target: &'a str,
    pub message: Option<&'a str>,
    pub timestamp: Timestamp,
    pub file: Option<&'a str>,
    pub line: Option<i64>,
    pub fields: Option<&'a str>,
}

/// Columns selected for an [`Event`], in [`row_to_event`] order.
const EVENT_COLUMNS: &str = "id, span_id, level, target, message, timestamp, file, line, fields";

/// Columns selected for a [`Span`], in [`row_to_span`] order.
const SPAN_COLUMNS: &str =
    "id, parent_id, name, level, target, file, line, started_at, ended_at, fields";

/// Repository over the structured log tables for query operations.
///
/// Clones share a single underlying connection. Use [`Storage::log_writer`] to
/// obtain a [`LogWriter`] with an isolated connection for batch insertion from
/// a background task.
///
/// [`Storage::log_writer`]: crate::Storage::log_writer
pub struct LogRepository {
    connection: Connection,
}

impl LogRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Starts building a span insert. Required fields are passed here; optional
    /// fields are set via builder methods. Call `execute().await` to persist.
    pub fn insert_span<'a>(
        &'a self,
        name: &'a str,
        level: Level,
        target: &'a str,
        started_at: Timestamp,
    ) -> SpanInsertBuilder<'a> {
        SpanInsertBuilder {
            repo: self,
            parent_id: None,
            name,
            level,
            target,
            file: None,
            line: None,
            started_at,
            fields: None,
        }
    }

    /// Starts building an event insert. Required fields are passed here;
    /// optional fields are set via builder methods. Call `execute().await` to
    /// persist.
    pub fn insert_event<'a>(
        &'a self,
        level: Level,
        target: &'a str,
        timestamp: Timestamp,
    ) -> EventInsertBuilder<'a> {
        EventInsertBuilder {
            repo: self,
            span_id: None,
            level,
            target,
            timestamp,
            message: None,
            file: None,
            line: None,
            fields: None,
        }
    }

    /// Records the end time of a span.
    pub async fn close_span(&self, id: i64, ended_at: Timestamp) -> Result<()> {
        self.connection
            .execute(
                SQL_CLOSE_SPAN,
                params_from_iter([
                    Value::Integer(ended_at.as_millisecond()),
                    Value::Integer(id),
                ]),
            )
            .await
            .map_err(database)?;
        Ok(())
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

        filters.like_any(&["message", "target", "fields"], filter.query.as_deref());

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
            Some(_) => {
                "SELECT DISTINCT target FROM events WHERE target LIKE ?1 ESCAPE '\\' ORDER BY \
                 target"
            }
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
    pub async fn list_spans(&self, filter: &SpanFilter, query: &ListQuery) -> Result<Page<Span>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::with_id("started_at", paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.raw(&format!("{LEVEL_RANK_SQL} >= {}", min_level.rank()));
        }

        match filter.parent {
            ParentFilter::Any => {}
            ParentFilter::RootOnly => filters.raw("parent_id IS NULL"),
            ParentFilter::ChildrenOf(id) => filters.eq_int("parent_id", Some(id)),
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

    /// Deletes events and finished spans older than `before`. Returns the
    /// number of deleted events. FTS entries are cleaned up by triggers.
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

/// Lists all distinct boot sessions, derived from the `boot_id` structured
/// field of stored events. Ordered newest first.
pub async fn list_boots(&self) -> Result<Vec<BootInfo>> {
    const SQL_LIST_BOOTS: &str = r#"
        SELECT json_extract(fields, '$.boot_id') AS boot_id,
               MIN(timestamp) AS first_seen,
               MAX(timestamp) AS last_seen,
               COUNT(*) AS event_count
        FROM events
        WHERE fields IS NOT NULL
          AND json_extract(fields, '$.boot_id') IS NOT NULL
        GROUP BY boot_id
        ORDER BY first_seen DESC
    "#;
    let mut rows = self
        .connection
        .query(SQL_LIST_BOOTS, params_from_iter(Vec::<Value>::new()))
        .await
        .map_err(database)?;
    let mut boots = Vec::new();
    while let Some(row) = rows.next().await.map_err(database)? {
        let text = match row.get_value(0).map_err(database)? {
            Value::Text(text) => text,
            _ => continue,
        };
        let Some(parsed) = Uuid::parse_str(&text).ok() else {
            continue;
        };
        boots.push(BootInfo {
            boot_id: parsed,
            first_seen: req_ts(&row, 1)?,
            last_seen: req_ts(&row, 2)?,
            event_count: req_int(&row, 3)?,
        });
    }
    Ok(boots)
}

    /// Inserts a synthetic event recording that log records were dropped.
    pub async fn record_dropped(&self, count: u64, timestamp: Timestamp) -> Result<()> {
        let fields =
            serde_json::to_string(&HashMap::from([("dropped", count)])).map_err(|err| {
                StorageError::Decode(format!("failed to serialize dropped fields: {err}"))
            })?;
        self.insert_event(Level::Warn, "aperture::log", timestamp)
            .message(Some(&format!(
                "dropped {count} log records due to full buffer"
            )))
            .fields(Some(&fields))
            .execute()
            .await?;
        Ok(())
    }
}

/// Builder for a span insert. Created by [`LogRepository::insert_span`].
pub struct SpanInsertBuilder<'a> {
    repo: &'a LogRepository,
    parent_id: Option<i64>,
    name: &'a str,
    level: Level,
    target: &'a str,
    file: Option<&'a str>,
    line: Option<i64>,
    started_at: Timestamp,
    fields: Option<&'a str>,
}

impl<'a> SpanInsertBuilder<'a> {
    pub fn parent_id(mut self, id: Option<i64>) -> Self {
        self.parent_id = id;
        self
    }

    pub fn file(mut self, file: Option<&'a str>) -> Self {
        self.file = file;
        self
    }

    pub fn line(mut self, line: Option<i64>) -> Self {
        self.line = line;
        self
    }

    pub fn fields(mut self, fields: Option<&'a str>) -> Self {
        self.fields = fields;
        self
    }

    pub async fn execute(self) -> Result<i64> {
        let params = params_from_iter([
            int_or_null(self.parent_id),
            Value::Text(self.name.to_owned()),
            Value::Text(self.level.as_db().to_owned()),
            Value::Text(self.target.to_owned()),
            text_ref_or_null(self.file),
            int_or_null(self.line),
            Value::Integer(self.started_at.as_millisecond()),
            text_ref_or_null(self.fields),
        ]);
        self.repo
            .connection
            .execute(SQL_INSERT_SPAN, params)
            .await
            .map_err(database)?;
        Ok(self.repo.connection.last_insert_rowid())
    }
}

/// Builder for an event insert. Created by [`LogRepository::insert_event`].
pub struct EventInsertBuilder<'a> {
    repo: &'a LogRepository,
    span_id: Option<i64>,
    level: Level,
    target: &'a str,
    timestamp: Timestamp,
    message: Option<&'a str>,
    file: Option<&'a str>,
    line: Option<i64>,
    fields: Option<&'a str>,
}

impl<'a> EventInsertBuilder<'a> {
    pub fn span_id(mut self, id: Option<i64>) -> Self {
        self.span_id = id;
        self
    }

    pub fn message(mut self, message: Option<&'a str>) -> Self {
        self.message = message;
        self
    }

    pub fn file(mut self, file: Option<&'a str>) -> Self {
        self.file = file;
        self
    }

    pub fn line(mut self, line: Option<i64>) -> Self {
        self.line = line;
        self
    }

    pub fn fields(mut self, fields: Option<&'a str>) -> Self {
        self.fields = fields;
        self
    }

    pub async fn execute(self) -> Result<i64> {
        let params = params_from_iter([
            int_or_null(self.span_id),
            Value::Text(self.level.as_db().to_owned()),
            Value::Text(self.target.to_owned()),
            text_ref_or_null(self.message),
            Value::Integer(self.timestamp.as_millisecond()),
            text_ref_or_null(self.file),
            int_or_null(self.line),
            text_ref_or_null(self.fields),
        ]);
        self.repo
            .connection
            .execute(SQL_INSERT_EVENT, params)
            .await
            .map_err(database)?;
        Ok(self.repo.connection.last_insert_rowid())
    }
}

/// Batch writer for structured logs. Owns a dedicated connection (independent
/// of the query connection) so it can be used from a background task without
/// conflicting with concurrent reads.
///
/// Prepared statements are reused across inserts for efficiency. Obtain one
/// with [`Storage::log_writer`].
///
/// [`Storage::log_writer`]: crate::Storage::log_writer
pub struct LogWriter {
    conn: Connection,
    insert_span: Statement,
    insert_event: Statement,
    close_span: Statement,
}

impl LogWriter {
    pub(crate) async fn new(conn: Connection) -> Result<Self> {
        let insert_span = conn.prepare(SQL_INSERT_SPAN).await.map_err(database)?;
        let insert_event = conn.prepare(SQL_INSERT_EVENT).await.map_err(database)?;
        let close_span = conn.prepare(SQL_CLOSE_SPAN).await.map_err(database)?;
        Ok(Self {
            conn,
            insert_span,
            insert_event,
            close_span,
        })
    }

    /// Inserts a span and returns its assigned id.
    pub async fn insert_span(&mut self, record: SpanRecord<'_>) -> Result<i64> {
        let params = params_from_iter([
            int_or_null(record.parent_id),
            Value::Text(record.name.to_owned()),
            Value::Text(record.level.as_db().to_owned()),
            Value::Text(record.target.to_owned()),
            text_ref_or_null(record.file),
            int_or_null(record.line),
            Value::Integer(record.started_at.as_millisecond()),
            text_ref_or_null(record.fields),
        ]);
        self.insert_span.execute(params).await.map_err(database)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Inserts a log event and returns its assigned id.
    pub async fn insert_event(&mut self, record: EventRecord<'_>) -> Result<i64> {
        let params = params_from_iter([
            int_or_null(record.span_id),
            Value::Text(record.level.as_db().to_owned()),
            Value::Text(record.target.to_owned()),
            text_ref_or_null(record.message),
            Value::Integer(record.timestamp.as_millisecond()),
            text_ref_or_null(record.file),
            int_or_null(record.line),
            text_ref_or_null(record.fields),
        ]);
        self.insert_event.execute(params).await.map_err(database)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Records the end time of a span.
    pub async fn close_span(&mut self, id: i64, ended_at: Timestamp) -> Result<()> {
        self.close_span
            .execute(params_from_iter([
                Value::Integer(ended_at.as_millisecond()),
                Value::Integer(id),
            ]))
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Inserts a synthetic event recording that log records were dropped.
    pub async fn record_dropped(&mut self, count: u64, timestamp: Timestamp) -> Result<()> {
        let fields =
            serde_json::to_string(&HashMap::from([("dropped", count)])).map_err(|err| {
                StorageError::Decode(format!("failed to serialize dropped fields: {err}"))
            })?;
        self.insert_event(EventRecord {
            span_id: None,
            level: Level::Warn,
            target: "aperture::log",
            message: Some(&format!("dropped {count} log records due to full buffer")),
            timestamp,
            file: None,
            line: None,
            fields: Some(&fields),
        })
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
