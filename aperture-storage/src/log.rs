//! Structured log storage: tracing spans and events persisted for querying.

use jiff::Timestamp;
use turso::transaction::Transaction;
use turso::{Connection, Statement, Value, params_from_iter};
use uuid::Uuid;

use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::page::{CursorValue, Keyset, ListQuery, Listing, Order, Page, Paginator};
use crate::query::Filters;
use crate::sql::{Columns, ToSql, get};

db_id! {
    /// Primary key of a row in the `log_spans` table.
    pub struct SpanId;
}

db_id! {
    /// Primary key of a row in the `log_events` table.
    pub struct LogEventId;
}

mod col {
    pub const BOOT_ID: &str = "boot_id";
    pub const ENDED_AT: &str = "ended_at";
    pub const FIELDS: &str = "fields";
    pub const FILE: &str = "file";
    pub const ID: &str = "id";
    pub const LEVEL: &str = "level";
    pub const LINE: &str = "line";
    pub const MESSAGE: &str = "message";
    pub const NAME: &str = "name";
    pub const PARENT_ID: &str = "parent_id";
    pub const SPAN_ID: &str = "span_id";
    pub const STARTED_AT: &str = "started_at";
    pub const TARGET: &str = "target";
    pub const TIMESTAMP: &str = "timestamp";
}

/// Columns selected for an [`LogEvent`], in [`LogEvent::try_from`] order.
const LOG_EVENT_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::SPAN_ID,
    col::LEVEL,
    col::TARGET,
    col::MESSAGE,
    col::TIMESTAMP,
    col::FILE,
    col::LINE,
    col::BOOT_ID,
    col::FIELDS,
]);

/// Columns selected for a [`Span`], in [`Span::try_from`] order.
const SPAN_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::PARENT_ID,
    col::NAME,
    col::LEVEL,
    col::TARGET,
    col::FILE,
    col::LINE,
    col::STARTED_AT,
    col::ENDED_AT,
    col::FIELDS,
]);

/// SQL shared between [`LogRepository`] and [`LogBatch`] for span inserts.
/// File-level because the parameter layout is a shared assumption.
const SQL_INSERT_SPAN: &str = sql!(
    INSERT INTO log_spans
    (tracing_id, parent_tracing_id, boot_id, name, level, target, file, line, started_at, fields)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
);

/// SQL shared between [`LogRepository`] and [`LogBatch`] for event inserts.
/// File-level because the parameter layout is a shared assumption.
const SQL_INSERT_EVENT: &str = sql!(
    INSERT INTO log_events
    (span_tracing_id, level, target, message, timestamp, file, line, boot_id, fields)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
);

/// SQL shared between [`LogRepository`] and [`LogBatch`] for span closes.
/// File-level because the parameter layout is a shared assumption.
const SQL_CLOSE_SPAN: &str =
    sql!(UPDATE log_spans SET ended_at = ?1 WHERE tracing_id = ?2 AND boot_id = ?3);

/// SQL for merging late-recorded field values into a span's existing fields.
const SQL_UPDATE_SPAN_FIELDS: &str = sql!(
    UPDATE log_spans
    SET fields = json_patch(fields, ?1)
    WHERE tracing_id = ?2 AND boot_id = ?3
);

/// Aggregate query that collapses events into one row per boot session.
const BOOT_AGGREGATE_SQL: &str = sql!(
    SELECT boot_id,
           MIN(timestamp) AS first_seen,
           MAX(timestamp) AS last_seen,
           COUNT(*) AS event_count
    FROM log_events
    WHERE boot_id IS NOT NULL
    GROUP BY boot_id
);

/// Severity level of a tracing event or span.
///
/// Stored as [`i64`] in the database (see `Level::as_db`). Higher values are
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
    pub(crate) const fn as_db(self) -> i64 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
        }
    }

    pub(crate) const fn from_db(value: i64) -> Result<Self> {
        match value {
            0 => Ok(Self::Trace),
            1 => Ok(Self::Debug),
            2 => Ok(Self::Info),
            3 => Ok(Self::Warn),
            4 => Ok(Self::Error),
            other => Err(StorageError::UnknownLogLevel(other)),
        }
    }
}

impl From<&tracing::Level> for Level {
    fn from(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::TRACE => Self::Trace,
            tracing::Level::DEBUG => Self::Debug,
            tracing::Level::INFO => Self::Info,
            tracing::Level::WARN => Self::Warn,
            tracing::Level::ERROR => Self::Error,
        }
    }
}

/// A persisted tracing span.
#[derive(Debug, Clone)]
pub struct Span {
    pub id: SpanId,
    pub parent_id: Option<SpanId>,
    pub name: String,
    pub level: Level,
    pub target: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// A persisted tracing event (log record).
#[derive(Debug, Clone)]
pub struct LogEvent {
    pub id: LogEventId,
    pub boot_id: Uuid,
    pub span_id: Option<SpanId>,
    pub level: Level,
    pub target: String,
    pub message: Option<String>,
    pub timestamp: Timestamp,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// One boot session observed in the log store.
#[derive(Debug, Clone)]
pub struct BootInfo {
    pub boot_id: Uuid,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub event_count: u64,
}

/// Filters for log event queries.
#[derive(Default)]
pub struct LogEventFilter {
    pub min_level: Option<Level>,
    pub target: Vec<String>,
    pub query: Option<String>,
    pub span_id: Option<SpanId>,
    pub boot_id: Option<Uuid>,
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
    /// Only root spans (`parent_id` IS NULL).
    RootOnly,
    /// Only direct children of the given span id.
    ChildrenOf(SpanId),
}

/// Filters for span queries.
#[derive(Default)]
pub struct SpanFilter {
    pub min_level: Option<Level>,
    pub target: Vec<String>,
    pub boot_id: Option<Uuid>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub parent: SpanParentFilter,
    pub fields: Vec<(String, String)>,
}

/// Plain-data description of a span to persist via [`LogBatch::insert_span`].
pub struct SpanRecord<'a> {
    pub tracing_id: u64,
    pub parent_tracing_id: Option<u64>,
    pub boot_id: Uuid,
    pub name: &'a str,
    pub level: Level,
    pub target: &'a str,
    pub file: Option<&'a str>,
    pub line: Option<u32>,
    pub started_at: Timestamp,
    pub fields: &'a serde_json::Map<String, serde_json::Value>,
}

/// Plain-data description of an event to persist via
/// [`LogBatch::insert_event`].
pub struct LogEventRecord<'a> {
    pub boot_id: Uuid,
    pub span_tracing_id: Option<u64>,
    pub level: Level,
    pub target: &'a str,
    pub message: Option<&'a str>,
    pub timestamp: Timestamp,
    pub file: Option<&'a str>,
    pub line: Option<u32>,
    pub fields: &'a serde_json::Map<String, serde_json::Value>,
}

/// Repository over the structured log tables for query operations.
pub struct LogRepository {
    connection: Connection,
}

impl LogRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Opens a batched write transaction over the log tables. All operations
    /// on the returned [`LogBatch`] are atomic: they commit together when
    /// [`LogBatch::commit`] is called, or roll back if the batch is dropped
    /// without committing.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the transaction or statement
    /// preparation fails.
    pub async fn batch(&self) -> Result<LogBatch<'_>> {
        let tx = self
            .connection
            .unchecked_transaction()
            .await
            .map_err(StorageError::from_turso)?;
        let insert_span = self
            .connection
            .prepare(SQL_INSERT_SPAN)
            .await
            .map_err(StorageError::from_turso)?;
        let insert_event = self
            .connection
            .prepare(SQL_INSERT_EVENT)
            .await
            .map_err(StorageError::from_turso)?;
        let close_span = self
            .connection
            .prepare(SQL_CLOSE_SPAN)
            .await
            .map_err(StorageError::from_turso)?;
        let update_span_fields = self
            .connection
            .prepare(SQL_UPDATE_SPAN_FIELDS)
            .await
            .map_err(StorageError::from_turso)?;
        Ok(LogBatch {
            tx,
            insert_span,
            insert_event,
            close_span,
            update_span_fields,
        })
    }

    /// Lists log events matching the given filters, ordered by timestamp
    /// descending by default.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn list_events(
        &self,
        filter: &LogEventFilter,
        query: &ListQuery,
    ) -> Result<Page<LogEvent>> {
        let paginator = Paginator::new(query, Order::Desc, Listing::LogEvents)?;
        let keyset = Keyset::with_id(col::TIMESTAMP, paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.gte_int(col::LEVEL, min_level.as_db());
        }

        filters.one_of(col::TARGET, filter.target.iter().map(String::as_str));
        filters.eq_int_opt(col::SPAN_ID, filter.span_id.map(SpanId::get));
        filters.eq_text_opt(
            col::BOOT_ID,
            filter.boot_id.as_ref().map(Uuid::to_string).as_deref(),
        );
        filters.gte_int_opt(
            col::TIMESTAMP,
            filter.since.map(jiff::Timestamp::as_microsecond),
        );
        filters.lte_int_opt(
            col::TIMESTAMP,
            filter.until.map(jiff::Timestamp::as_microsecond),
        );

        for (key, value) in &filter.fields {
            filters.json_path_eq(col::FIELDS, key, value);
        }

        filters.like_any_opt(&[col::MESSAGE, col::TARGET], filter.query.as_deref());

        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM log_events_resolved {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = LOG_EVENT_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            items.push(LogEvent::try_from(&row)?);
        }
        Ok(paginator.finish(items, |event| {
            (
                CursorValue::Int(event.timestamp.as_microsecond()),
                CursorValue::Int(event.id.get()),
            )
        }))
    }

    /// Lists distinct targets across both events and spans, optionally
    /// filtered by prefix, paginated by target (ascending by default).
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a target
    /// cannot be read.
    #[tracing::instrument(level = "info", skip(self, query))]
    /// Lists distinct targets across both events and spans, optionally
    /// filtered by prefix, paginated by target (ascending by default).
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a target
    /// cannot be read.
    #[tracing::instrument(level = "info", skip(self, query))]
    pub async fn list_targets(&self, q: Option<&str>, query: &ListQuery) -> Result<Page<String>> {
        let paginator = Paginator::new(query, Order::Asc, Listing::LogTargets)?;
        let keyset = Keyset::unique(col::TARGET, paginator.query_order());

        let mut filters = Filters::new();
        filters.like_prefix(col::TARGET, q.unwrap_or(""));
        filters.keyset(&keyset, &paginator);

        let where_clause = filters.where_clause();
        let sql = format!(
            "SELECT {col} FROM log_events {where_clause} UNION SELECT {col} FROM log_spans \
             {where_clause} ORDER BY {order} LIMIT {limit}",
            col = col::TARGET,
            where_clause = where_clause,
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut targets = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            targets.push(get(&row, 0)?);
        }
        Ok(paginator.finish(targets, |t| {
            (CursorValue::Text(t.clone()), CursorValue::Int(0))
        }))
    }

    /// Lists spans matching the given filters, ordered by `started_at`
    /// descending by default.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn list_spans(&self, filter: &SpanFilter, query: &ListQuery) -> Result<Page<Span>> {
        let paginator = Paginator::new(query, Order::Desc, Listing::LogSpans)?;
        let keyset = Keyset::with_id(col::STARTED_AT, paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.gte_int(col::LEVEL, min_level.as_db());
        }

        match filter.parent {
            SpanParentFilter::Any => {}
            SpanParentFilter::RootOnly => filters.raw("parent_id IS NULL"),
            SpanParentFilter::ChildrenOf(id) => filters.eq_int(col::PARENT_ID, id.get()),
        }

        filters.one_of(col::TARGET, filter.target.iter().map(String::as_str));
        filters.eq_text_opt(
            col::BOOT_ID,
            filter.boot_id.as_ref().map(Uuid::to_string).as_deref(),
        );
        filters.gte_int_opt(
            col::STARTED_AT,
            filter.since.map(jiff::Timestamp::as_microsecond),
        );
        filters.lte_int_opt(
            col::STARTED_AT,
            filter.until.map(jiff::Timestamp::as_microsecond),
        );
        for (key, value) in &filter.fields {
            filters.json_path_eq(col::FIELDS, key, value);
        }
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM log_spans_resolved {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = SPAN_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            items.push(Span::try_from(&row)?);
        }
        Ok(paginator.finish(items, |span| {
            (
                CursorValue::Int(span.started_at.as_microsecond()),
                CursorValue::Int(span.id.get()),
            )
        }))
    }

    /// Returns a single span by id, if it exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get_span(&self, id: SpanId) -> Result<Option<Span>> {
        let sql = format!(
            sql!(SELECT {cols} FROM log_spans_resolved WHERE id = ?1),
            cols = SPAN_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(id.get())]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(Span::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns all events belonging to `span_id`, ordered by timestamp.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn events_for_span(&self, span_id: SpanId) -> Result<Vec<LogEvent>> {
        let sql = format!(
            sql!(SELECT {cols} FROM log_events_resolved WHERE span_id = ?1 ORDER BY timestamp),
            cols = LOG_EVENT_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(span_id.get())]))
            .await
            .map_err(StorageError::from_turso)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            items.push(LogEvent::try_from(&row)?);
        }
        Ok(items)
    }

    /// Closes every span of `boot_id` that is still open by setting its
    /// `ended_at` to the given timestamp. Spans of other boots are left
    /// untouched: a span left open by a crashed boot must not be stamped
    /// with an unrelated shutdown time. Returns the number of rows updated.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
    pub async fn close_open_spans(&self, boot_id: Uuid, ended_at: Timestamp) -> Result<u64> {
        self.connection
            .execute(
                sql!(UPDATE log_spans SET ended_at = ?1 WHERE ended_at IS NULL AND boot_id = ?2),
                params_from_iter([Value::Integer(ended_at.as_microsecond()), boot_id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)
    }

    /// Deletes events and finished spans older than `before`. Returns the
    /// number of deleted events.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if either delete fails.
    pub async fn prune_before(&self, before: Timestamp) -> Result<u64> {
        let micros = before.as_microsecond();
        let event_count = self
            .connection
            .execute(
                sql!(DELETE FROM log_events WHERE timestamp < ?1),
                params_from_iter([Value::Integer(micros)]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        self.connection
            .execute(
                sql!(DELETE FROM log_spans WHERE ended_at IS NOT NULL AND ended_at < ?1),
                params_from_iter([Value::Integer(micros)]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(event_count)
    }

    /// Lists distinct boot sessions, derived from the `boot_id` column of
    /// stored events. Paginated by `first_seen` (descending by default).
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
    #[tracing::instrument(level = "info", skip(self, query))]
    pub async fn list_boots(&self, query: &ListQuery) -> Result<Page<BootInfo>> {
        let paginator = Paginator::new(query, Order::Desc, Listing::LogBoots)?;
        let keyset = Keyset::unique("first_seen", paginator.query_order());

        let mut filters = Filters::new();
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            "SELECT * FROM ({inner}) {where_clause} ORDER BY {order} LIMIT {limit}",
            inner = BOOT_AGGREGATE_SQL,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut boots = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            boots.push(BootInfo {
                boot_id: get(&row, 0)?,
                first_seen: get(&row, 1)?,
                last_seen: get(&row, 2)?,
                event_count: get(&row, 3)?,
            });
        }
        Ok(paginator.finish(boots, |boot| {
            (
                CursorValue::Int(boot.first_seen.as_microsecond()),
                CursorValue::Int(0),
            )
        }))
    }
}

/// Batched write transaction over the log tables.
///
/// Wraps a database transaction with cached prepared statements so a batch of
/// span/event operations runs as a single atomic commit. Obtain one with
/// [`LogRepository::batch`].
pub struct LogBatch<'conn> {
    tx: Transaction<'conn>,
    insert_span: Statement,
    insert_event: Statement,
    close_span: Statement,
    update_span_fields: Statement,
}

impl LogBatch<'_> {
    /// Inserts a span.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    pub async fn insert_span(&mut self, record: SpanRecord<'_>) -> Result<()> {
        let params = params_from_iter([
            record.tracing_id.to_sql(),
            record.parent_tracing_id.to_sql(),
            record.boot_id.to_sql(),
            record.name.to_sql(),
            record.level.to_sql(),
            record.target.to_sql(),
            record.file.to_sql(),
            record.line.to_sql(),
            record.started_at.to_sql(),
            record.fields.to_sql(),
        ]);
        self.insert_span
            .execute(params)
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Inserts a log event.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    pub async fn insert_event(&mut self, record: LogEventRecord<'_>) -> Result<()> {
        let params = params_from_iter([
            record.span_tracing_id.to_sql(),
            record.level.to_sql(),
            record.target.to_sql(),
            record.message.to_sql(),
            record.timestamp.to_sql(),
            record.file.to_sql(),
            record.line.to_sql(),
            record.boot_id.to_sql(),
            record.fields.to_sql(),
        ]);
        self.insert_event
            .execute(params)
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Records the end time of a span identified by its `tracing_id`.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
    pub async fn close_span(
        &mut self,
        tracing_id: u64,
        boot_id: Uuid,
        ended_at: Timestamp,
    ) -> Result<()> {
        self.close_span
            .execute(params_from_iter([
                ended_at.to_sql(),
                tracing_id.to_sql(),
                boot_id.to_sql(),
            ]))
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Merges late-recorded field values into a span's existing fields.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
    pub async fn update_span_fields(
        &mut self,
        tracing_id: u64,
        boot_id: Uuid,
        fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        self.update_span_fields
            .execute(params_from_iter([
                fields.to_sql(),
                tracing_id.to_sql(),
                boot_id.to_sql(),
            ]))
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Inserts a synthetic event recording that log records were dropped.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    pub async fn record_dropped(
        &mut self,
        count: u64,
        timestamp: Timestamp,
        boot_id: Uuid,
    ) -> Result<()> {
        let mut fields = serde_json::Map::with_capacity(1);
        fields.insert(
            "dropped".to_owned(),
            serde_json::Value::Number(count.into()),
        );
        self.insert_event(LogEventRecord {
            span_tracing_id: None,
            level: Level::Warn,
            target: "aperture::log",
            message: Some("dropped log records due to full buffer"),
            timestamp,
            file: None,
            line: None,
            boot_id,
            fields: &fields,
        })
        .await?;
        Ok(())
    }

    /// Commits all pending operations. Consumes the batch.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the commit fails.
    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await.map_err(StorageError::from_turso)
    }
}

impl TryFrom<&turso::Row> for LogEvent {
    type Error = StorageError;

    fn try_from(row: &turso::Row) -> Result<Self> {
        Ok(Self {
            id: LOG_EVENT_COLUMNS.extract(row, col::ID)?,
            span_id: LOG_EVENT_COLUMNS.extract(row, col::SPAN_ID)?,
            level: LOG_EVENT_COLUMNS.extract(row, col::LEVEL)?,
            target: LOG_EVENT_COLUMNS.extract(row, col::TARGET)?,
            message: LOG_EVENT_COLUMNS.extract(row, col::MESSAGE)?,
            timestamp: LOG_EVENT_COLUMNS.extract(row, col::TIMESTAMP)?,
            file: LOG_EVENT_COLUMNS.extract(row, col::FILE)?,
            line: LOG_EVENT_COLUMNS.extract(row, col::LINE)?,
            boot_id: LOG_EVENT_COLUMNS.extract(row, col::BOOT_ID)?,
            fields: LOG_EVENT_COLUMNS.extract(row, col::FIELDS)?,
        })
    }
}

impl TryFrom<&turso::Row> for Span {
    type Error = StorageError;

    fn try_from(row: &turso::Row) -> Result<Self> {
        Ok(Self {
            id: SPAN_COLUMNS.extract(row, col::ID)?,
            parent_id: SPAN_COLUMNS.extract(row, col::PARENT_ID)?,
            name: SPAN_COLUMNS.extract(row, col::NAME)?,
            level: SPAN_COLUMNS.extract(row, col::LEVEL)?,
            target: SPAN_COLUMNS.extract(row, col::TARGET)?,
            file: SPAN_COLUMNS.extract(row, col::FILE)?,
            line: SPAN_COLUMNS.extract(row, col::LINE)?,
            started_at: SPAN_COLUMNS.extract(row, col::STARTED_AT)?,
            ended_at: SPAN_COLUMNS.extract(row, col::ENDED_AT)?,
            fields: SPAN_COLUMNS.extract(row, col::FIELDS)?,
        })
    }
}
