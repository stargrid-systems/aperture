//! Structured log storage: tracing spans and events persisted for querying.

use jiff::Timestamp;
use serde_json::Map;
use turso::transaction::Transaction;
use turso::{Connection, Statement, Value, params_from_iter};
use uuid::Uuid;

use crate::error::{Result, StorageError, database};
use crate::id::DbId;
use crate::macros::sql;
use crate::page::{CursorValue, Filters, Keyset, ListQuery, Order, Page, Paginator, escape_like};
use crate::row::{
    int_or_null, json_map, map_ref_or_null, opt_db_id, opt_text, opt_ts, opt_u32, opt_uuid,
    req_db_id, req_int, req_text, req_ts, req_u64, text_ref_or_null, uuid_or_null,
};

/// Columns selected for an [`Event`], in [`row_to_event`] order.
const EVENT_COLUMNS: &str =
    "id, span_id, level, target, message, timestamp, file, line, boot_id, fields";

/// Columns selected for a [`Span`], in [`row_to_span`] order.
const SPAN_COLUMNS: &str =
    "id, parent_id, name, level, target, file, line, started_at, ended_at, fields";

mod col {
    pub const FIELDS: &str = "fields";
    pub const LEVEL: &str = "level";
    pub const MESSAGE: &str = "message";
    pub const PARENT_ID: &str = "parent_id";
    pub const SPAN_ID: &str = "span_id";
    pub const STARTED_AT: &str = "started_at";
    pub const TARGET: &str = "target";
    pub const TIMESTAMP: &str = "timestamp";
}

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
            other => Err(StorageError::UnknownLogLevel(other)),
        }
    }
}

/// A persisted tracing span.
#[derive(Debug, Clone)]
pub struct Span {
    pub id: DbId,
    pub parent_id: Option<DbId>,
    pub name: String,
    pub level: Level,
    pub target: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub fields: Map<String, serde_json::Value>,
}

/// A persisted tracing event (log record).
#[derive(Debug, Clone)]
pub struct Event {
    pub id: DbId,
    pub span_id: Option<DbId>,
    pub level: Level,
    pub target: String,
    pub message: Option<String>,
    pub timestamp: Timestamp,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub boot_id: Option<Uuid>,
    pub fields: Map<String, serde_json::Value>,
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
pub struct EventFilter {
    pub min_level: Option<Level>,
    pub target: Vec<String>,
    pub query: Option<String>,
    pub span_id: Option<DbId>,
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
    ChildrenOf(DbId),
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
    pub fields: Option<&'a serde_json::Map<String, serde_json::Value>>,
}

/// Plain-data description of an event to persist via
/// [`LogBatch::insert_event`].
pub struct EventRecord<'a> {
    pub span_tracing_id: Option<u64>,
    pub level: Level,
    pub target: &'a str,
    pub message: Option<&'a str>,
    pub timestamp: Timestamp,
    pub file: Option<&'a str>,
    pub line: Option<u32>,
    pub boot_id: Option<Uuid>,
    pub fields: Option<&'a serde_json::Map<String, serde_json::Value>>,
}

/// Repository over the structured log tables for query operations.
pub struct LogRepository {
    connection: Connection,
}

impl LogRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Opens a batched write transaction over the log tables. All operations
    /// on the returned [`LogBatch`] are atomic: they commit together when
    /// [`LogBatch::commit`] is called, or roll back if the batch is dropped
    /// without committing.
    pub async fn batch(&self) -> Result<LogBatch<'_>> {
        let tx = self
            .connection
            .unchecked_transaction()
            .await
            .map_err(database)?;
        let insert_span = self
            .connection
            .prepare(SQL_INSERT_SPAN)
            .await
            .map_err(database)?;
        let insert_event = self
            .connection
            .prepare(SQL_INSERT_EVENT)
            .await
            .map_err(database)?;
        let close_span = self
            .connection
            .prepare(SQL_CLOSE_SPAN)
            .await
            .map_err(database)?;
        let update_span_fields = self
            .connection
            .prepare(SQL_UPDATE_SPAN_FIELDS)
            .await
            .map_err(database)?;
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
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn list_events(
        &self,
        filter: &EventFilter,
        query: &ListQuery,
    ) -> Result<Page<Event>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::with_id(col::TIMESTAMP, paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.gte_int(col::LEVEL, Some(min_level.as_db()));
        }

        filters.one_of(col::TARGET, filter.target.iter().map(String::as_str));
        filters.eq_int(col::SPAN_ID, filter.span_id.map(DbId::get));
        filters.gte_int(col::TIMESTAMP, filter.since.map(|ts| ts.as_millisecond()));
        filters.lte_int(col::TIMESTAMP, filter.until.map(|ts| ts.as_millisecond()));

        for (key, value) in &filter.fields {
            filters.json_path_eq(col::FIELDS, key, value);
        }

        filters.like_any(&[col::MESSAGE, col::TARGET], filter.query.as_deref());

        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM log_events_resolved {where_clause} ORDER BY {order} LIMIT {limit}),
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
            (
                CursorValue::Int(event.timestamp.as_millisecond()),
                event.id.get(),
            )
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
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn list_spans(&self, filter: &SpanFilter, query: &ListQuery) -> Result<Page<Span>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::with_id(col::STARTED_AT, paginator.query_order());

        let mut filters = Filters::new();

        if let Some(min_level) = filter.min_level {
            filters.gte_int(col::LEVEL, Some(min_level.as_db()));
        }

        match filter.parent {
            SpanParentFilter::Any => {}
            SpanParentFilter::RootOnly => filters.raw("parent_id IS NULL"),
            SpanParentFilter::ChildrenOf(id) => filters.eq_int(col::PARENT_ID, Some(id.get())),
        }

        filters.one_of(col::TARGET, filter.target.iter().map(String::as_str));
        filters.gte_int(col::STARTED_AT, filter.since.map(|ts| ts.as_millisecond()));
        filters.lte_int(col::STARTED_AT, filter.until.map(|ts| ts.as_millisecond()));
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
            .map_err(database)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            items.push(row_to_span(&row)?);
        }
        Ok(paginator.finish(items, |span| {
            (
                CursorValue::Int(span.started_at.as_millisecond()),
                span.id.get(),
            )
        }))
    }

    /// Returns a single span by id, if it exists.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get_span(&self, id: DbId) -> Result<Option<Span>> {
        let sql = format!(
            sql!(SELECT {cols} FROM log_spans_resolved WHERE id = ?1),
            cols = SPAN_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(id.get())]))
            .await
            .map_err(database)?;
        match rows.next().await.map_err(database)? {
            Some(row) => Ok(Some(row_to_span(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns all events belonging to `span_id`, ordered by timestamp.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn events_for_span(&self, span_id: DbId) -> Result<Vec<Event>> {
        let sql = format!(
            sql!(SELECT {cols} FROM log_events_resolved WHERE span_id = ?1 ORDER BY timestamp),
            cols = EVENT_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(span_id.get())]))
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

    /// Lists all distinct boot sessions, derived from the `boot_id` column of
    /// stored events. Ordered newest first.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_boots(&self) -> Result<Vec<BootInfo>> {
        const SQL_LIST_BOOTS: &str = sql!(
            SELECT boot_id,
                   MIN(timestamp) AS first_seen,
                   MAX(timestamp) AS last_seen,
                   COUNT(*) AS event_count
            FROM log_events
            WHERE boot_id IS NOT NULL
            GROUP BY boot_id
            ORDER BY first_seen DESC
        );
        let mut rows = self
            .connection
            .query(SQL_LIST_BOOTS, params_from_iter(Vec::<Value>::new()))
            .await
            .map_err(database)?;
        let mut boots = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            let bytes = match row.get_value(0).map_err(database)? {
                Value::Blob(bytes) => bytes,
                _ => continue,
            };
            let Ok(parsed) = Uuid::from_slice(&bytes) else {
                continue;
            };
            boots.push(BootInfo {
                boot_id: parsed,
                first_seen: req_ts(&row, 1)?,
                last_seen: req_ts(&row, 2)?,
                event_count: req_u64(&row, 3)?,
            });
        }
        Ok(boots)
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

impl<'conn> LogBatch<'conn> {
    /// Inserts a span.
    pub async fn insert_span(&mut self, record: SpanRecord<'_>) -> Result<()> {
        let params = params_from_iter([
            Value::Integer(record.tracing_id as i64),
            record
                .parent_tracing_id
                .map_or(Value::Null, |v| Value::Integer(v as i64)),
            Value::Blob(record.boot_id.as_bytes().to_vec()),
            Value::Text(record.name.to_owned()),
            Value::Integer(record.level.as_db()),
            Value::Text(record.target.to_owned()),
            text_ref_or_null(record.file),
            int_or_null(record.line),
            Value::Integer(record.started_at.as_millisecond()),
            map_ref_or_null(record.fields),
        ]);
        self.insert_span.execute(params).await.map_err(database)?;
        Ok(())
    }

    /// Inserts a log event.
    pub async fn insert_event(&mut self, record: EventRecord<'_>) -> Result<()> {
        let params = params_from_iter([
            record
                .span_tracing_id
                .map_or(Value::Null, |v| Value::Integer(v as i64)),
            Value::Integer(record.level.as_db()),
            Value::Text(record.target.to_owned()),
            text_ref_or_null(record.message),
            Value::Integer(record.timestamp.as_millisecond()),
            text_ref_or_null(record.file),
            int_or_null(record.line),
            uuid_or_null(record.boot_id),
            map_ref_or_null(record.fields),
        ]);
        self.insert_event.execute(params).await.map_err(database)?;
        Ok(())
    }

    /// Records the end time of a span identified by its tracing_id.
    pub async fn close_span(
        &mut self,
        tracing_id: u64,
        boot_id: Uuid,
        ended_at: Timestamp,
    ) -> Result<()> {
        self.close_span
            .execute(params_from_iter([
                Value::Integer(ended_at.as_millisecond()),
                Value::Integer(tracing_id as i64),
                Value::Blob(boot_id.as_bytes().to_vec()),
            ]))
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Merges late-recorded field values into a span's existing fields.
    pub async fn update_span_fields(
        &mut self,
        tracing_id: u64,
        boot_id: Uuid,
        fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let json = serde_json::to_string(fields)
            .expect("serializing a JSON map produced by serde_json cannot fail");
        self.update_span_fields
            .execute(params_from_iter([
                Value::Text(json),
                Value::Integer(tracing_id as i64),
                Value::Blob(boot_id.as_bytes().to_vec()),
            ]))
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Inserts a synthetic event recording that log records were dropped.
    pub async fn record_dropped(
        &mut self,
        count: u64,
        timestamp: Timestamp,
        boot_id: Uuid,
    ) -> Result<()> {
        let mut fields = serde_json::Map::new();
        fields.insert(
            "dropped".to_owned(),
            serde_json::Value::Number(count.into()),
        );
        self.insert_event(EventRecord {
            span_tracing_id: None,
            level: Level::Warn,
            target: "aperture::log",
            message: Some("dropped log records due to full buffer"),
            timestamp,
            file: None,
            line: None,
            boot_id: Some(boot_id),
            fields: Some(&fields),
        })
        .await?;
        Ok(())
    }

    /// Commits all pending operations. Consumes the batch.
    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await.map_err(database)
    }
}

fn row_to_event(row: &turso::Row) -> Result<Event> {
    Ok(Event {
        id: req_db_id(row, 0)?,
        span_id: opt_db_id(row, 1)?,
        level: Level::from_db(req_int(row, 2)?)?,
        target: req_text(row, 3)?,
        message: opt_text(row, 4)?,
        timestamp: req_ts(row, 5)?,
        file: opt_text(row, 6)?,
        line: opt_u32(row, 7)?,
        boot_id: opt_uuid(row, 8)?,
        fields: json_map(row, 9)?,
    })
}

fn row_to_span(row: &turso::Row) -> Result<Span> {
    Ok(Span {
        id: req_db_id(row, 0)?,
        parent_id: opt_db_id(row, 1)?,
        name: req_text(row, 2)?,
        level: Level::from_db(req_int(row, 3)?)?,
        target: req_text(row, 4)?,
        file: opt_text(row, 5)?,
        line: opt_u32(row, 6)?,
        started_at: req_ts(row, 7)?,
        ended_at: opt_ts(row, 8)?,
        fields: json_map(row, 9)?,
    })
}
