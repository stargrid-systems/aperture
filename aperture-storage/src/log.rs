//! Structured log storage: tracing spans and events persisted for querying.

use std::collections::HashMap;

use jiff::Timestamp;
use turso::{Connection, Statement, Value, params_from_iter};
use uuid::Uuid;

use crate::error::{Result, StorageError, database};
use crate::macros::sql;
use crate::page::{CursorValue, Filters, Keyset, ListQuery, Order, Page, Paginator, escape_like};
use crate::row::{
    int_or_null, opt_int, opt_text, opt_ts, opt_u32, req_int, req_text, req_ts, text_ref_or_null,
};

/// Columns selected for an [`Event`], in [`row_to_event`] order.
const EVENT_COLUMNS: &str = "id, span_id, level, target, message, timestamp, file, line, fields";

/// Columns selected for a [`Span`], in [`row_to_span`] order.
const SPAN_COLUMNS: &str =
    "id, parent_id, name, level, target, file, line, started_at, ended_at, fields";

/// SQL shared between [`LogRepository`] and [`LogWriter`] for span inserts.
/// File-level because the parameter layout is a shared assumption.
const SQL_INSERT_SPAN: &str = sql!(
    INSERT INTO log_spans
    (parent_id, name, level, target, file, line, started_at, fields)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
);

/// SQL shared between [`LogRepository`] and [`LogWriter`] for event inserts.
/// File-level because the parameter layout is a shared assumption.
const SQL_INSERT_EVENT: &str = sql!(
    INSERT INTO log_events
    (span_id, level, target, message, timestamp, file, line, fields)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
);

/// SQL shared between [`LogRepository`] and [`LogWriter`] for span closes.
/// File-level because the parameter layout is a shared assumption.
const SQL_CLOSE_SPAN: &str = sql!(UPDATE log_spans SET ended_at = ?1 WHERE id = ?2);

/// SQL for updating span fields after late-recorded values arrive.
const SQL_UPDATE_SPAN_FIELDS: &str = sql!(UPDATE log_spans SET fields = ?1 WHERE id = ?2);

/// Severity level of a tracing event or span.
///
/// Stored as [`i64`] in the database (see [`Level::as_db`]). Higher values are
/// more severe, so `level >= N` filters by minimum severity without a CASE
/// expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// Numeric severity rank stored in the database. Higher is more severe.
    pub(crate) fn as_db(self) -> i64 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
        }
    }

    pub(crate) fn from_db(value: i64) -> Result<Self> {
        match value {
            0 => Ok(Self::Trace),
            1 => Ok(Self::Debug),
            2 => Ok(Self::Info),
            3 => Ok(Self::Warn),
            4 => Ok(Self::Error),
            other => Err(StorageError::Decode(format!("unknown log level {other}"))),
        }
    }
}

/// A persisted tracing span.
#[derive(Debug, Clone)]
pub struct Span {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub level: Level,
    pub target: String,
    pub file: Option<String>,
    pub line: Option<u32>,
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
    pub line: Option<u32>,
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
    pub target: Vec<String>,
    pub query: Option<String>,
    pub span_id: Option<i64>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub fields: Vec<(String, String)>,
}

/// Filter for the `parent_id` column of a span query.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpanParentFilter {
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
    pub target: Vec<String>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub parent: SpanParentFilter,
    pub fields: Vec<(String, String)>,
}

/// Plain-data description of a span to persist via [`LogWriter::insert_span`].
pub struct SpanRecord<'a> {
    pub parent_id: Option<i64>,
    pub name: &'a str,
    pub level: Level,
    pub target: &'a str,
    pub file: Option<&'a str>,
    pub line: Option<u32>,
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
    pub line: Option<u32>,
    pub fields: Option<&'a str>,
}

/// Repository over the structured log tables for query operations.
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
    #[tracing::instrument(level = "info", skip(self, filter, query))]
    pub async fn list_events(
        &self,
        filter: &EventFilter,
        query: &ListQuery,
    ) -> Result<Page<Event>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::with_id("timestamp", paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.gte_int("level", Some(min_level.as_db()));
        }

        let targets: Vec<&str> = filter.target.iter().map(String::as_str).collect();
        filters.one_of("target", &targets);
        filters.eq_int("span_id", filter.span_id);
        filters.gte_int("timestamp", filter.since.map(|ts| ts.as_millisecond()));
        filters.lte_int("timestamp", filter.until.map(|ts| ts.as_millisecond()));

        for (key, value) in &filter.fields {
            filters.json_path_eq("fields", key, value);
        }

        filters.like_any(&["message", "target", "fields"], filter.query.as_deref());

        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM log_events {where_clause} ORDER BY {order} LIMIT {limit}),
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

    /// Lists distinct targets across both events and spans, optionally
    /// filtered by prefix.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_targets(&self, q: Option<&str>) -> Result<Vec<String>> {
        // Raw string because sql!() cannot handle SQL single-quoted literals.
        let sql = match q {
            Some(_) => {
                r#"
                SELECT target FROM log_events WHERE target LIKE ?1 ESCAPE '\'
                UNION
                SELECT target FROM log_spans WHERE target LIKE ?1 ESCAPE '\'
                ORDER BY target
            "#
            }
            None => sql!(
                SELECT target FROM log_events
                UNION
                SELECT target FROM log_spans
                ORDER BY target
            ),
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
    #[tracing::instrument(level = "info", skip(self, filter, query))]
    pub async fn list_spans(&self, filter: &SpanFilter, query: &ListQuery) -> Result<Page<Span>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::with_id("started_at", paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.gte_int("level", Some(min_level.as_db()));
        }

        match filter.parent {
            SpanParentFilter::Any => {}
            SpanParentFilter::RootOnly => filters.raw("parent_id IS NULL"),
            SpanParentFilter::ChildrenOf(id) => filters.eq_int("parent_id", Some(id)),
        }

        let targets: Vec<&str> = filter.target.iter().map(String::as_str).collect();
        filters.one_of("target", &targets);
        filters.gte_int("started_at", filter.since.map(|ts| ts.as_millisecond()));
        filters.lte_int("started_at", filter.until.map(|ts| ts.as_millisecond()));
        for (key, value) in &filter.fields {
            filters.json_path_eq("fields", key, value);
        }
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM log_spans {where_clause} ORDER BY {order} LIMIT {limit}),
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
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get_span(&self, id: i64) -> Result<Option<Span>> {
        let sql = format!(
            sql!(SELECT {cols} FROM log_spans WHERE id = ?1),
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
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn events_for_span(&self, span_id: i64) -> Result<Vec<Event>> {
        let sql = format!(
            sql!(SELECT {cols} FROM log_events WHERE span_id = ?1 ORDER BY timestamp),
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

    /// Closes every span that is still open by setting its `ended_at` to the
    /// given timestamp. Returns the number of rows updated.
    pub async fn close_open_spans(&self, ended_at: Timestamp) -> Result<u64> {
        self.connection
            .execute(
                sql!(UPDATE log_spans SET ended_at = ?1 WHERE ended_at IS NULL),
                params_from_iter([Value::Integer(ended_at.as_millisecond())]),
            )
            .await
            .map_err(database)
    }

    /// Deletes events and finished spans older than `before`. Returns the
    /// number of deleted events.
    pub async fn prune_before(&self, before: Timestamp) -> Result<u64> {
        let millis = before.as_millisecond();
        let event_count = self
            .connection
            .execute(
                sql!(DELETE FROM log_events WHERE timestamp < ?1),
                params_from_iter([Value::Integer(millis)]),
            )
            .await
            .map_err(database)?;
        self.connection
            .execute(
                sql!(DELETE FROM log_spans WHERE ended_at IS NOT NULL AND ended_at < ?1),
                params_from_iter([Value::Integer(millis)]),
            )
            .await
            .map_err(database)?;
        Ok(event_count)
    }

    /// Lists all distinct boot sessions, derived from the `boot_id` structured
    /// field of stored events. Ordered newest first.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_boots(&self) -> Result<Vec<BootInfo>> {
        // Raw string because sql!() cannot handle SQL single-quoted literals.
        const SQL_LIST_BOOTS: &str = r#"
            SELECT json_extract(fields, '$.boot_id') AS boot_id,
                   MIN(timestamp) AS first_seen,
                   MAX(timestamp) AS last_seen,
                   COUNT(*) AS event_count
            FROM log_events
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
    line: Option<u32>,
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

    pub fn line(mut self, line: Option<u32>) -> Self {
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
            Value::Integer(self.level.as_db()),
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
    line: Option<u32>,
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

    pub fn line(mut self, line: Option<u32>) -> Self {
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
            Value::Integer(self.level.as_db()),
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

/// Batch writer for structured logs. Holds a [`Connection`] for batch inserts
/// from a background task.
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
    update_span_fields: Statement,
}

impl LogWriter {
    pub(crate) async fn new(conn: Connection) -> Result<Self> {
        let insert_span = conn.prepare(SQL_INSERT_SPAN).await.map_err(database)?;
        let insert_event = conn.prepare(SQL_INSERT_EVENT).await.map_err(database)?;
        let close_span = conn.prepare(SQL_CLOSE_SPAN).await.map_err(database)?;
        let update_span_fields = conn
            .prepare(SQL_UPDATE_SPAN_FIELDS)
            .await
            .map_err(database)?;
        Ok(Self {
            conn,
            insert_span,
            insert_event,
            close_span,
            update_span_fields,
        })
    }

    /// Inserts a span and returns its assigned id.
    pub async fn insert_span(&mut self, record: SpanRecord<'_>) -> Result<i64> {
        let params = params_from_iter([
            int_or_null(record.parent_id),
            Value::Text(record.name.to_owned()),
            Value::Integer(record.level.as_db()),
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
            Value::Integer(record.level.as_db()),
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

    /// Updates the fields JSON of a span. Used when fields are recorded after
    /// the span was created via `Span::record`.
    pub async fn update_span_fields(&mut self, id: i64, fields: &str) -> Result<()> {
        self.update_span_fields
            .execute(params_from_iter([
                Value::Text(fields.to_owned()),
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

fn row_to_event(row: &turso::Row) -> Result<Event> {
    Ok(Event {
        id: req_int(row, 0)?,
        span_id: opt_int(row, 1)?,
        level: Level::from_db(req_int(row, 2)?)?,
        target: req_text(row, 3)?,
        message: opt_text(row, 4)?,
        timestamp: req_ts(row, 5)?,
        file: opt_text(row, 6)?,
        line: opt_u32(row, 7)?,
        fields: opt_text(row, 8)?,
    })
}

fn row_to_span(row: &turso::Row) -> Result<Span> {
    Ok(Span {
        id: req_int(row, 0)?,
        parent_id: opt_int(row, 1)?,
        name: req_text(row, 2)?,
        level: Level::from_db(req_int(row, 3)?)?,
        target: req_text(row, 4)?,
        file: opt_text(row, 5)?,
        line: opt_u32(row, 6)?,
        started_at: req_ts(row, 7)?,
        ended_at: opt_ts(row, 8)?,
        fields: opt_text(row, 9)?,
    })
}
