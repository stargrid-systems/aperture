//! Login sessions: token hashes with sliding expiry, backed by the database.

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
use crate::secret::TokenHash;
use crate::sql::{Columns, ToSql};

/// Primary key of a row in the `sessions` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schema", schema(value_type = String))]
pub struct SessionId(DbId);

impl SessionId {
    pub const fn get(self) -> i64 {
        self.0.get()
    }
}

impl From<i64> for SessionId {
    fn from(value: i64) -> Self {
        Self(DbId::from(value))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = ParseIntError;
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        s.parse::<i64>().map(|v| Self(DbId::from(v)))
    }
}

impl Serialize for SessionId {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SessionId {
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
    pub const EXPIRES_AT: &str = "expires_at";
    pub const ID: &str = "id";
    pub const TOKEN_HASH: &str = "token_hash";
}

const SESSION_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::ACTOR_ID,
    col::TOKEN_HASH,
    col::EXPIRES_AT,
    col::CREATED_AT,
]);

/// A login session associated with an actor.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// Store-assigned id.
    pub id: SessionId,
    /// The authenticated actor.
    pub actor_id: ActorId,
    /// SHA-256 hash of the session token.
    pub token_hash: TokenHash,
    /// When the session expires.
    pub expires_at: Timestamp,
    /// When the session was created.
    pub created_at: Timestamp,
}

/// Repository over the sessions table.
pub struct SessionRepository {
    connection: Connection,
}

impl SessionRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new session and returns its assigned id.
    #[tracing::instrument(level = "info", skip(self, token_hash))]
    pub async fn create(
        &self,
        actor_id: ActorId,
        token_hash: &TokenHash,
        expires_at: Timestamp,
        created_at: Timestamp,
    ) -> Result<SessionId> {
        let params = params_from_iter([
            actor_id.to_sql(),
            token_hash.to_sql(),
            expires_at.to_sql(),
            created_at.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO sessions (actor_id, token_hash, expires_at, created_at)
                    VALUES (?1, ?2, ?3, ?4)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(SessionId::from(self.connection.last_insert_rowid()))
    }

    /// Returns the session with `token_hash`, if one exists.
    #[tracing::instrument(level = "info", skip(self, token_hash))]
    pub async fn find_by_token_hash(&self, token_hash: &TokenHash) -> Result<Option<Session>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM sessions WHERE token_hash = ?1),
            cols = SESSION_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([token_hash.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_session(&row)?)),
            None => Ok(None),
        }
    }

    /// Extends the expiry of session `id`. Used for sliding expiry.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn touch_expiry(&self, id: SessionId, expires_at: Timestamp) -> Result<()> {
        self.connection
            .execute(
                sql!(UPDATE sessions SET expires_at = ?1 WHERE id = ?2),
                params_from_iter([expires_at.to_sql(), id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Deletes the session with `id`. Used for logout.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete(&self, id: SessionId) -> Result<()> {
        self.connection
            .execute(
                sql!(DELETE FROM sessions WHERE id = ?1),
                params_from_iter([id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Deletes all sessions that have expired before `now`. Returns how many
    /// were removed.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete_expired(&self, now: Timestamp) -> Result<usize> {
        let affected = self
            .connection
            .execute(
                sql!(DELETE FROM sessions WHERE expires_at < ?1),
                params_from_iter([now.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(affected as usize)
    }

    /// Deletes all sessions for `actor_id`. Used when disabling an actor or
    /// forcing logout everywhere.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete_for_actor(&self, actor_id: ActorId) -> Result<usize> {
        let affected = self
            .connection
            .execute(
                sql!(DELETE FROM sessions WHERE actor_id = ?1),
                params_from_iter([actor_id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(affected as usize)
    }

    /// Lists sessions for `actor_id`, ordered by creation time descending.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_for_actor(&self, actor_id: ActorId) -> Result<Vec<Session>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM sessions WHERE actor_id = ?1 ORDER BY created_at DESC),
            cols = SESSION_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([actor_id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            sessions.push(row_to_session(&row)?);
        }
        Ok(sessions)
    }
}

fn row_to_session(row: &Row) -> Result<Session> {
    Ok(Session {
        id: SESSION_COLUMNS.extract(row, col::ID)?,
        actor_id: SESSION_COLUMNS.extract(row, col::ACTOR_ID)?,
        token_hash: SESSION_COLUMNS.extract(row, col::TOKEN_HASH)?,
        expires_at: SESSION_COLUMNS.extract(row, col::EXPIRES_AT)?,
        created_at: SESSION_COLUMNS.extract(row, col::CREATED_AT)?,
    })
}
