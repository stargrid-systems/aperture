//! Login sessions: token hashes with sliding expiry, backed by the database.

use jiff::Timestamp;
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::secret::TokenHash;
use crate::sql::{Columns, ToSql};

db_id! {
    /// Primary key of a row in the `sessions` table.
    pub struct SessionId;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new session and returns its assigned id.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
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
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
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
            Some(row) => Ok(Some(Session::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Extends the expiry of session `id`. Used for sliding expiry.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
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
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the delete fails.
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
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the delete fails.
    ///
    /// # Panics
    ///
    /// Never panics in practice. The affected row count fits `usize`.
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
        Ok(usize::try_from(affected).expect("row count fits usize"))
    }

    /// Deletes all sessions for `actor_id`. Used when disabling an actor or
    /// forcing logout everywhere.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the delete fails.
    ///
    /// # Panics
    ///
    /// Never panics in practice. The affected row count fits `usize`.
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
        Ok(usize::try_from(affected).expect("row count fits usize"))
    }

    /// Deletes all sessions for `actor_id` except the one whose token hash
    /// matches `keep`. When `keep` is `None`, every session is deleted. Used on
    /// password change to revoke other sessions while keeping the caller in.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the delete fails.
    ///
    /// # Panics
    ///
    /// Never panics in practice. The affected row count fits `usize`.
    #[tracing::instrument(level = "info", skip(self, keep))]
    pub async fn delete_for_actor_except(
        &self,
        actor_id: ActorId,
        keep: Option<&TokenHash>,
    ) -> Result<usize> {
        let affected = match keep {
            Some(hash) => self
                .connection
                .execute(
                    sql!(DELETE FROM sessions WHERE actor_id = ?1 AND token_hash != ?2),
                    params_from_iter([actor_id.to_sql(), hash.to_sql()]),
                )
                .await
                .map_err(StorageError::from_turso)?,
            None => return self.delete_for_actor(actor_id).await,
        };
        Ok(usize::try_from(affected).expect("row count fits usize"))
    }

    /// Lists sessions for `actor_id`, ordered by creation time descending.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
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
            sessions.push(Session::try_from(&row)?);
        }
        Ok(sessions)
    }
}

impl TryFrom<&Row> for Session {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
            id: SESSION_COLUMNS.extract(row, col::ID)?,
            actor_id: SESSION_COLUMNS.extract(row, col::ACTOR_ID)?,
            token_hash: SESSION_COLUMNS.extract(row, col::TOKEN_HASH)?,
            expires_at: SESSION_COLUMNS.extract(row, col::EXPIRES_AT)?,
            created_at: SESSION_COLUMNS.extract(row, col::CREATED_AT)?,
        })
    }
}
