//! Domain events: persisted records of significant state changes.

use std::fmt;
use std::result::Result as StdResult;
use std::str::FromStr;

use jiff::Timestamp;
use serde::de::{Error as DeError, Visitor};
use serde_json::Value;
use turso::transaction::Transaction;
use turso::{Connection, Row, Statement, params_from_iter};
use uuid::Uuid;

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::sql;
use crate::page::{CursorValue, Keyset, ListQuery, Listing, Order, Page, Paginator};
use crate::query::Filters;
use crate::sql::{Columns, FromSql, ToSql};

/// Primary key of a row in the `events` table.
///
/// A `UUIDv7` assigned at emit time, so the id is known before the row is
/// persisted by the recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, utoipa::ToSchema)]
#[schema(value_type = String, format = Uuid)]
pub struct EventId(Uuid);

impl EventId {
    /// Generates a fresh time-ordered id.
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Returns the underlying UUID.
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for EventId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for EventId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl serde::Serialize for EventId {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for EventId {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct IdVisitor;

        impl Visitor<'_> for IdVisitor {
            type Value = EventId;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a UUID string")
            }

            fn visit_str<E>(self, v: &str) -> StdResult<Self::Value, E>
            where
                E: DeError,
            {
                v.parse().map_err(DeError::custom)
            }
        }

        deserializer.deserialize_str(IdVisitor)
    }
}

impl ToSql for EventId {
    fn to_sql(&self) -> turso::Value {
        self.0.to_sql()
    }
}

impl FromSql for EventId {
    fn from_sql(value: turso::Value, idx: usize) -> Result<Self> {
        Uuid::from_sql(value, idx).map(Self)
    }
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

/// SQL shared between [`EventRepository`] and [`EventBatch`]. File-level
/// because the parameter layout is a shared assumption.
const SQL_INSERT_EVENT: &str = sql!(
    INSERT INTO events (id, key, data, actor, timestamp)
    VALUES (?1, ?2, ?3, ?4, ?5)
);

/// A persisted domain event.
#[derive(Debug, Clone)]
pub struct Event {
    pub id: EventId,
    pub key: String,
    pub data: Value,
    pub actor: ActorId,
    pub timestamp: Timestamp,
}

/// Input for creating an event. The id is assigned at emit time.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub id: EventId,
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

    /// Creates a new event. The id must be unique.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] if the insert fails.
    #[tracing::instrument(level = "info", skip(self, new))]
    pub async fn create(&self, new: &NewEvent) -> Result<()> {
        let params = params_from_iter([
            new.id.to_sql(),
            new.key.to_sql(),
            new.data.to_sql(),
            new.actor.to_sql(),
            new.timestamp.to_sql(),
        ]);
        self.connection
            .execute(SQL_INSERT_EVENT, params)
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Opens a transaction that batch-inserts events.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the transaction or statement preparation
    /// fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn batch(&self) -> Result<EventBatch<'_>> {
        let tx = self
            .connection
            .unchecked_transaction()
            .await
            .map_err(StorageError::from_turso)?;
        let insert = self
            .connection
            .prepare(SQL_INSERT_EVENT)
            .await
            .map_err(StorageError::from_turso)?;
        Ok(EventBatch { tx, insert })
    }

    /// Returns the event with `id`, if it exists.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the query fails or the row cannot be
    /// decoded.
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
    /// Returns [`StorageError`] if the query or cursor is invalid, or a row
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
                CursorValue::Text(event.id.to_string()),
            )
        }))
    }
}

/// A transaction that inserts events in batches. Produced by
/// [`EventRepository::batch`].
pub struct EventBatch<'conn> {
    tx: Transaction<'conn>,
    insert: Statement,
}

impl EventBatch<'_> {
    /// Queues one event insert.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] if the insert fails. Any error
    /// poisons the transaction; drop it without committing to roll back.
    pub async fn insert(&mut self, new: &NewEvent) -> Result<()> {
        let params = params_from_iter([
            new.id.to_sql(),
            new.key.to_sql(),
            new.data.to_sql(),
            new.actor.to_sql(),
            new.timestamp.to_sql(),
        ]);
        self.insert
            .execute(params)
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Commits the batch.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the commit fails.
    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await.map_err(StorageError::from_turso)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::get;

    /// Runs `EXPLAIN QUERY PLAN` on `sql` and returns the detail rows.
    async fn explain(repo: &EventRepository, sql: &str, params: Vec<turso::Value>) -> Vec<String> {
        let mut rows = repo
            .connection
            .query(
                &format!("EXPLAIN QUERY PLAN {sql}"),
                params_from_iter(params),
            )
            .await
            .unwrap();
        let mut plans = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            plans.push(get::<String>(&row, 3).unwrap());
        }
        plans
    }

    /// turso cannot satisfy `ORDER BY timestamp` from an index because the
    /// `timestamp_us` custom type sorts as an encoded blob, so the listing
    /// still sorts and no plan test can demand otherwise. What the composite
    /// indexes do serve today is the key-equality seek, pinned here.
    #[tokio::test]
    async fn key_filtered_listing_uses_composite_index() {
        let storage = crate::Storage::open(":memory:").await.unwrap();
        let repo = storage.events().unwrap();

        let keyset = Keyset::with_id(col::TIMESTAMP, Order::Desc);
        // The keyset predicate a second page carries, hand-written to match
        // Keyset::condition output for a Desc listing.
        let cond = format!(
            "({col} < ?2 OR ({col} = ?2 AND id < ?3))",
            col = col::TIMESTAMP,
        );
        let sql = format!(
            sql!(SELECT {cols} FROM events WHERE key = ?1 AND {cond} ORDER BY {order} LIMIT {limit}),
            cols = EVENT_COLUMNS,
            cond = cond,
            order = keyset.order_by(),
            limit = ListQuery::DEFAULT_LIMIT + 1,
        );
        let params = vec![
            turso::Value::Text("artifact.written".to_owned()),
            turso::Value::Integer(1_000),
            turso::Value::Text(EventId::generate().to_string()),
        ];
        let plans = explain(&repo, &sql, params).await;
        let joined = plans.join("\n");
        assert!(
            joined.contains("idx_events_key_timestamp_id"),
            "key filter must seek through the composite index: {plans:?}"
        );
    }
}
