//! API keys: long-lived tokens for headless clients, with per-key scoping.

use std::time::Duration;

use jiff::Timestamp;
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::page::{CursorValue, Keyset, ListQuery, Order, Page, Paginator};
use crate::query::Filters;
use crate::secret::ApiKeyHash;
use crate::sql::{Columns, ToSql};

/// Minimum spacing between `last_used_at` updates. See
/// [`ApiKeyRepository::touch_last_used_if_stale`].
const STALE_THRESHOLD: Duration = Duration::from_secs(60);

db_id! {
    /// Primary key of a row in the `api_keys` table.
    pub struct ApiKeyId;
}

mod col {
    pub const ACTOR_ID: &str = "actor_id";
    pub const CREATED_AT: &str = "created_at";
    pub const ID: &str = "id";
    pub const KEY_HASH: &str = "key_hash";
    pub const LAST_USED_AT: &str = "last_used_at";
    pub const NAME: &str = "name";
    pub const PREFIX: &str = "prefix";
}

const API_KEY_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::ACTOR_ID,
    col::NAME,
    col::KEY_HASH,
    col::PREFIX,
    col::LAST_USED_AT,
    col::CREATED_AT,
]);

/// A stored API key. The raw key is never stored, only its SHA-256 hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    /// Store-assigned id.
    pub id: ApiKeyId,
    /// The actor this key authenticates as.
    pub actor_id: ActorId,
    /// Human-readable name.
    pub name: String,
    /// SHA-256 hash of the full key string.
    pub key_hash: ApiKeyHash,
    /// First characters of the key, for display and lookup.
    pub prefix: String,
    /// When the key was last used, if ever.
    pub last_used_at: Option<Timestamp>,
    /// When the key was created.
    pub created_at: Timestamp,
}

/// Repository over the `api_keys` table.
pub struct ApiKeyRepository {
    connection: Connection,
}

impl ApiKeyRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new API key record and returns it.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    #[tracing::instrument(level = "info", skip(self, key_hash))]
    pub async fn create(
        &self,
        actor_id: ActorId,
        name: &str,
        key_hash: &ApiKeyHash,
        prefix: &str,
        created_at: Timestamp,
    ) -> Result<ApiKey> {
        let params = params_from_iter([
            actor_id.to_sql(),
            name.to_sql(),
            key_hash.to_sql(),
            prefix.to_sql(),
            created_at.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO api_keys (actor_id, name, key_hash, prefix, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        let id = ApiKeyId::from(self.connection.last_insert_rowid());
        Ok(ApiKey {
            id,
            actor_id,
            name: name.to_owned(),
            key_hash: key_hash.clone(),
            prefix: prefix.to_owned(),
            last_used_at: None,
            created_at,
        })
    }

    /// Returns the API key with matching `prefix`, if one exists. The caller
    /// must still verify the full key hash.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM api_keys WHERE prefix = ?1),
            cols = API_KEY_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([prefix.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(ApiKey::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns the API key with `id`, if one exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: ApiKeyId) -> Result<Option<ApiKey>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM api_keys WHERE id = ?1),
            cols = API_KEY_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(ApiKey::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists API keys for `actor_id`, paginated by id (newest first by
    /// default).
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
    #[tracing::instrument(level = "info", skip(self, query))]
    pub async fn list_for_actor(
        &self,
        actor_id: ActorId,
        query: &ListQuery,
    ) -> Result<Page<ApiKey>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::unique(col::ID, paginator.query_order());

        let mut filters = Filters::new();
        filters.eq_int(col::ACTOR_ID, actor_id.get());
        filters.keyset(&keyset, &paginator);

        let sql_str = format!(
            sql!(SELECT {cols} FROM api_keys {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = API_KEY_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut keys = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            keys.push(ApiKey::try_from(&row)?);
        }
        Ok(paginator.finish(keys, |key| {
            (
                CursorValue::Int(key.id.get()),
                CursorValue::Int(key.id.get()),
            )
        }))
    }

    /// Updates the last-used timestamp only if it is unset or older than the
    /// stale threshold (60 seconds) from `at`. This bounds the per-request
    /// write load from API-key authentication to at most one write per window.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn touch_last_used_if_stale(&self, id: ApiKeyId, at: Timestamp) -> Result<()> {
        let cutoff = at - STALE_THRESHOLD;
        self.connection
            .execute(
                sql!(
                    UPDATE api_keys SET last_used_at = ?1
                    WHERE id = ?2 AND (last_used_at IS NULL OR last_used_at < ?3)
                ),
                params_from_iter([at.to_sql(), id.to_sql(), cutoff.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Deletes the API key with `id`. Does nothing if absent.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the delete fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete(&self, id: ApiKeyId) -> Result<()> {
        self.connection
            .execute(
                sql!(DELETE FROM api_keys WHERE id = ?1),
                params_from_iter([id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }
}

impl TryFrom<&Row> for ApiKey {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
            id: API_KEY_COLUMNS.extract(row, col::ID)?,
            actor_id: API_KEY_COLUMNS.extract(row, col::ACTOR_ID)?,
            name: API_KEY_COLUMNS.extract(row, col::NAME)?,
            key_hash: API_KEY_COLUMNS.extract(row, col::KEY_HASH)?,
            prefix: API_KEY_COLUMNS.extract(row, col::PREFIX)?,
            last_used_at: API_KEY_COLUMNS.extract(row, col::LAST_USED_AT)?,
            created_at: API_KEY_COLUMNS.extract(row, col::CREATED_AT)?,
        })
    }
}
