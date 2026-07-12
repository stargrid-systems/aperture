//! API keys: long-lived tokens for headless clients, with per-key scoping.

use std::fmt;
use std::num::ParseIntError;
use std::result::Result as StdResult;
use std::str::FromStr;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::id::DbId;
use crate::macros::sql;
use crate::secret::ApiKeyHash;
use crate::sql::{Columns, ToSql};

/// Primary key of a row in the `api_keys` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schema", schema(value_type = String))]
pub struct ApiKeyId(DbId);

impl ApiKeyId {
    pub const fn get(self) -> i64 {
        self.0.get()
    }
}

impl From<i64> for ApiKeyId {
    fn from(value: i64) -> Self {
        Self(DbId::from(value))
    }
}

impl fmt::Display for ApiKeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ApiKeyId {
    type Err = ParseIntError;
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        s.parse::<i64>().map(|v| Self(DbId::from(v)))
    }
}

impl Serialize for ApiKeyId {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ApiKeyId {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        DbId::deserialize(deserializer).map(Self)
    }
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
#[derive(Debug, Clone, PartialEq)]
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

/// Repository over the api_keys table.
pub struct ApiKeyRepository {
    connection: Connection,
}

impl ApiKeyRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new API key record and returns it.
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
            Some(row) => Ok(Some(row_to_api_key(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns the API key with `id`, if one exists.
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
            Some(row) => Ok(Some(row_to_api_key(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists API keys for `actor_id`, ordered by creation time descending.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_for_actor(&self, actor_id: ActorId) -> Result<Vec<ApiKey>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM api_keys WHERE actor_id = ?1 ORDER BY created_at DESC),
            cols = API_KEY_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([actor_id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        let mut keys = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            keys.push(row_to_api_key(&row)?);
        }
        Ok(keys)
    }

    /// Updates the last-used timestamp for key `id`.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn touch_last_used(&self, id: ApiKeyId, at: Timestamp) -> Result<()> {
        self.connection
            .execute(
                sql!(UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2),
                params_from_iter([at.to_sql(), id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Deletes the API key with `id`. Does nothing if absent.
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

fn row_to_api_key(row: &Row) -> Result<ApiKey> {
    Ok(ApiKey {
        id: API_KEY_COLUMNS.extract(row, col::ID)?,
        actor_id: API_KEY_COLUMNS.extract(row, col::ACTOR_ID)?,
        name: API_KEY_COLUMNS.extract(row, col::NAME)?,
        key_hash: API_KEY_COLUMNS.extract(row, col::KEY_HASH)?,
        prefix: API_KEY_COLUMNS.extract(row, col::PREFIX)?,
        last_used_at: API_KEY_COLUMNS.extract(row, col::LAST_USED_AT)?,
        created_at: API_KEY_COLUMNS.extract(row, col::CREATED_AT)?,
    })
}
