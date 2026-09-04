//! Domain events: persisted records of significant state changes.

use jiff::Timestamp;
use serde_json::Value;
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::page::{CursorValue, Keyset, ListQuery, Listing, Order, Page, Paginator};
use crate::query::Filters;
use crate::sql::{Columns, ToSql};

db_id! {
    /// Primary key of a row in the `events` table.
    pub struct EventId;
}

mod col {
    pub const ACTOR: &str = "actor";
    pub const DATA: &str = "data";
    pub const ID: &str = "id";
    pub const KEY: &str = "key";
    pub const TIMESTAMP: &str = "timestamp";
}

const EVENT_COLUMNS: Columns =
    Columns::new(&[col::ID, col::KEY, col::DATA, col::ACTOR, col::TIMESTAMP]);

/// A persisted domain event.
#[derive(Debug, Clone)]
pub struct Event {
    pub id: EventId,
    pub key: String,
    pub data: Value,
    pub actor: ActorId,
    pub timestamp: Timestamp,
}

/// Input for creating an event.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub key: String,
    pub data: Value,
    pub actor: ActorId,
    pub timestamp: Timestamp,
}

/// Filters for event queries.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub key: Option<String>,
    pub key_prefix: Option<String>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
}

pub struct EventRepository {
    connection: Connection,
}

impl EventRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new event and returns its assigned id.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    #[tracing::instrument(level = "info", skip(self, new))]
    pub async fn create(&self, new: &NewEvent) -> Result<EventId> {
        let params = params_from_iter([
            new.key.to_sql(),
            new.data.to_sql(),
            new.actor.to_sql(),
            new.timestamp.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO events (key, data, actor, timestamp)
                    VALUES (?1, ?2, ?3, ?4)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(EventId::from(self.connection.last_insert_rowid()))
    }

    /// Returns the event with `id`, if it exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: EventId) -> Result<Option<Event>> {
        let sql = format!(
            sql!(SELECT {cols} FROM events WHERE id = ?1),
            cols = EVENT_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(Event::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists events matching the given filters, ordered by timestamp
    /// descending by default.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn list(&self, filter: &EventFilter, query: &ListQuery) -> Result<Page<Event>> {
        let paginator = Paginator::new(query, Order::Desc, Listing::Events)?;
        let keyset = Keyset::with_id(col::TIMESTAMP, paginator.query_order());

        let mut filters = Filters::new();
        filters.eq_text_opt(col::KEY, filter.key.as_deref());
        if let Some(prefix) = &filter.key_prefix {
            filters.like_prefix(col::KEY, prefix);
        }
        filters.gte_int_opt(
            col::TIMESTAMP,
            filter.since.map(jiff::Timestamp::as_microsecond),
        );
        filters.lte_int_opt(
            col::TIMESTAMP,
            filter.until.map(jiff::Timestamp::as_microsecond),
        );
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
            .map_err(StorageError::from_turso)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            items.push(Event::try_from(&row)?);
        }
        Ok(paginator.finish(items, |event| {
            (
                CursorValue::Int(event.timestamp.as_microsecond()),
                event.id.get(),
            )
        }))
    }
}

impl TryFrom<&Row> for Event {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
            id: EVENT_COLUMNS.extract(row, col::ID)?,
            key: EVENT_COLUMNS.extract(row, col::KEY)?,
            data: EVENT_COLUMNS.extract(row, col::DATA)?,
            actor: EVENT_COLUMNS.extract(row, col::ACTOR)?,
            timestamp: EVENT_COLUMNS.extract(row, col::TIMESTAMP)?,
        })
    }
}
