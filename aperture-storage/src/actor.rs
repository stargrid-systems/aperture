//! Actors: the identity behind every action in the system.
//!
//! Users are one kind of actor, but there are others: API keys authenticate as
//! a separate actor, and internal system tasks run under a system actor. Every
//! task invocation records its initiator so the system always knows who caused
//! what.

use jiff::Timestamp;
use turso::{Connection, Row, params_from_iter};

use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::sql::{Columns, ToSql, get};

db_id! {
    /// Primary key of a row in the `actors` table.
    ///
    /// Used wherever an actor is referenced: `users.actor_id`,
    /// `sessions.actor_id`, `api_keys.actor_id`, `tasks.initiator_id`.
    pub struct ActorId;
}

impl ActorId {
    /// The well-known system actor (id 1). Seeded by the initial migration.
    pub const SYSTEM: Self = Self::from_i64(1);
}

mod col {
    pub const CREATED_AT: &str = "created_at";
    pub const DISABLED_AT: &str = "disabled_at";
    pub const DISPLAY_NAME: &str = "display_name";
    pub const ID: &str = "id";
    pub const KIND: &str = "kind";
}

const ACTOR_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::KIND,
    col::DISPLAY_NAME,
    col::CREATED_AT,
    col::DISABLED_AT,
]);

/// What kind of identity an actor represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// A human user with credentials.
    User,
    /// An API key used by headless clients.
    ApiKey,
    /// Internal system processes.
    System,
}

impl ActorKind {
    pub(crate) const fn as_db(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ApiKey => "api_key",
            Self::System => "system",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "user" => Ok(Self::User),
            "api_key" => Ok(Self::ApiKey),
            "system" => Ok(Self::System),
            other => Err(StorageError::UnknownActorKind(other.to_owned())),
        }
    }
}

/// An identity that can cause actions in the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// Store-assigned id.
    pub id: ActorId,
    /// What kind of identity this is.
    pub kind: ActorKind,
    /// Human-readable label.
    pub display_name: String,
    /// When the actor was created.
    pub created_at: Timestamp,
    /// When the actor was disabled, if it was.
    pub disabled_at: Option<Timestamp>,
}

/// Repository over the actor table.
pub struct ActorRepository {
    connection: Connection,
}

impl ActorRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new actor and returns the full record.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn create(
        &self,
        kind: ActorKind,
        display_name: &str,
        created_at: Timestamp,
    ) -> Result<Actor> {
        let params = params_from_iter([kind.to_sql(), display_name.to_sql(), created_at.to_sql()]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO actors (kind, display_name, created_at)
                    VALUES (?1, ?2, ?3)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        let id = ActorId::from(self.connection.last_insert_rowid());
        Ok(Actor {
            id,
            kind,
            display_name: display_name.to_owned(),
            created_at,
            disabled_at: None,
        })
    }

    /// Returns the actor with `id`, if it exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: ActorId) -> Result<Option<Actor>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM actors WHERE id = ?1),
            cols = ACTOR_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(Actor::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Marks the actor as disabled at `at`. Does nothing if already disabled.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn disable(&self, id: ActorId, at: Timestamp) -> Result<()> {
        self.connection
            .execute(
                sql!(UPDATE actors SET disabled_at = ?1 WHERE id = ?2 AND disabled_at IS NULL),
                params_from_iter([at.to_sql(), id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Returns how many actors exist of any kind. Used at bootstrap to detect
    /// first run.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the count cannot be read.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn count(&self) -> Result<i64> {
        let mut rows = self
            .connection
            .query(sql!(SELECT COUNT(*) FROM actors), ())
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(get(&row, 0)?),
            None => Ok(0),
        }
    }

    /// Lists actors of `kind`, ordered by creation time.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_by_kind(&self, kind: ActorKind) -> Result<Vec<Actor>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM actors WHERE kind = ?1 ORDER BY created_at),
            cols = ACTOR_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([kind.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        let mut actors = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            actors.push(Actor::try_from(&row)?);
        }
        Ok(actors)
    }
}

impl TryFrom<&Row> for Actor {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
            id: ACTOR_COLUMNS.extract(row, col::ID)?,
            kind: ACTOR_COLUMNS.extract(row, col::KIND)?,
            display_name: ACTOR_COLUMNS.extract(row, col::DISPLAY_NAME)?,
            created_at: ACTOR_COLUMNS.extract(row, col::CREATED_AT)?,
            disabled_at: ACTOR_COLUMNS.extract(row, col::DISABLED_AT)?,
        })
    }
}
