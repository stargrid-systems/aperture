//! Storage layer for the Aperture gateway.
//!
//! A small repository abstraction over an embedded turso (SQLite-compatible)
//! database. Engine details stay inside this crate, so the rest of Aperture
//! depends only on the domain types exposed here. That keeps the engine
//! swappable behind this boundary.

use std::time::Duration;

use jiff::Timestamp;
use turso::transaction::{Transaction, TransactionBehavior};
use turso::{Builder, Connection, Database, params_from_iter};

pub use self::actor::{Actor, ActorId, ActorKind, ActorRepository};
pub use self::api_key::{ApiKey, ApiKeyId, ApiKeyRepository};
pub use self::artifact::{Artifact, ArtifactId, ArtifactKeyEntry, ArtifactRepository, VersionSort};
pub use self::digest::{Digest, DigestAlgorithm, InvalidDigest};
pub use self::error::{Result, StorageError};
pub use self::interval::{Interval, InvalidInterval};
pub use self::key::{ArtifactKey, InvalidArtifactKey, MAX_LEN as ARTIFACT_KEY_MAX_LEN};
pub use self::log::{
    BootInfo, Event, EventFilter, EventId, EventRecord, Level, LogBatch, LogRepository, Span,
    SpanFilter, SpanId, SpanParentFilter, SpanRecord,
};
pub use self::media_type::{InvalidMediaType, MediaType};
pub use self::page::{Cursor, CursorValue, ListQuery, Listing, Order, Page, Paginator};
pub use self::role::{RoleAssignment, RoleAssignmentRepository, SubjectKind};
pub use self::secret::{ApiKeyHash, PasswordHash, TokenHash};
pub use self::session::{Session, SessionId, SessionRepository};
pub use self::setting::{SettingRecord, SettingRepository};
pub use self::task::{
    InvalidJsonPath, JsonField, JsonFilter, JsonPath, ParentFilter, StatusFilter, TaskId,
    TaskInvocation, TaskRepository, TaskStatus,
};
pub use self::task_schedule::{
    NewTaskSchedule, TaskSchedule, TaskScheduleId, TaskSchedulePatch, TaskScheduleRepository,
};
pub use self::user::{User, UserId, UserRepository};
use crate::macros::sql;
use crate::sql::{ToSql, get};

mod actor;
mod api_key;
mod artifact;
mod digest;
mod error;
mod interval;
mod key;
mod log;
mod macros;
mod media_type;
mod migration;
mod page;
mod query;
mod role;
mod secret;
mod serde_util;
mod session;
mod setting;
mod sql;
mod task;
mod task_schedule;
mod user;

/// Busy timeout for write contention. turso uses WAL mode by default, but two
/// writers still need to take turns. Without a timeout the second writer
/// immediately receives "database is locked".
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Handle to the gateway's persistent storage.
///
/// Cloning is cheap: [`Database`] is a single [`Arc`] handle internally. Each
/// repository gets its own independent [`Connection`] via
/// [`Database::connect`], so concurrent queries do not serialize through a
/// shared connection's locks.
///
/// [`Arc`]: std::sync::Arc
#[derive(Clone)]
pub struct Storage {
    db: Database,
}

impl Storage {
    /// Opens the database at `path`, creating it if needed, and applies any
    /// pending migrations.
    ///
    /// Pass `":memory:"` for an ephemeral in-memory database.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the database cannot be opened, connected to,
    /// or migrated.
    pub async fn open(path: &str) -> Result<Self> {
        let db = Builder::new_local(path)
            .experimental_index_method(true)
            .experimental_custom_types(true)
            .build()
            .await
            .map_err(StorageError::from_turso)?;
        let conn = db.connect().map_err(StorageError::from_turso)?;
        conn.busy_timeout(BUSY_TIMEOUT)
            .map_err(StorageError::from_turso)?;
        migration::run(&conn).await?;
        Ok(Self { db })
    }

    /// Creates a fresh independent connection with the busy timeout applied.
    fn connect(&self) -> Result<Connection> {
        let conn = self.db.connect().map_err(StorageError::from_turso)?;
        conn.busy_timeout(BUSY_TIMEOUT)
            .map_err(StorageError::from_turso)?;
        Ok(conn)
    }

    /// Returns the repository over the artifact catalog.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn artifacts(&self) -> Result<ArtifactRepository> {
        Ok(ArtifactRepository::new(self.connect()?))
    }

    /// Returns the repository over the task catalog.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn tasks(&self) -> Result<TaskRepository> {
        Ok(TaskRepository::new(self.connect()?))
    }

    /// Returns the repository over the periodic task schedules.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn task_schedules(&self) -> Result<TaskScheduleRepository> {
        Ok(TaskScheduleRepository::new(self.connect()?))
    }

    /// Returns the repository over the `settings` table.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn settings(&self) -> Result<SettingRepository> {
        Ok(SettingRepository::new(self.connect()?))
    }

    /// Returns the repository over the structured log tables.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn logs(&self) -> Result<LogRepository> {
        Ok(LogRepository::new(self.connect()?))
    }

    /// Returns the repository over the actors table.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn actors(&self) -> Result<ActorRepository> {
        Ok(ActorRepository::new(self.connect()?))
    }

    /// Returns the repository over the users table.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn users(&self) -> Result<UserRepository> {
        Ok(UserRepository::new(self.connect()?))
    }

    /// Returns the repository over the sessions table.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn sessions(&self) -> Result<SessionRepository> {
        Ok(SessionRepository::new(self.connect()?))
    }

    /// Returns the repository over the `api_keys` table.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn api_keys(&self) -> Result<ApiKeyRepository> {
        Ok(ApiKeyRepository::new(self.connect()?))
    }

    /// Returns the repository over the role assignment table.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if a new connection cannot be opened.
    pub fn role_assignments(&self) -> Result<RoleAssignmentRepository> {
        Ok(RoleAssignmentRepository::new(self.connect()?))
    }

    /// Creates a user actor and user record in one transaction. If the user
    /// insert fails (e.g. duplicate username), the actor insert is rolled back.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the connection, transaction, or either insert
    /// fails.
    pub async fn create_user(
        &self,
        username: &str,
        password_hash: &PasswordHash,
        password_change_required_at: Option<Timestamp>,
        now: Timestamp,
    ) -> Result<(Actor, User)> {
        let conn = self.connect()?;
        let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate)
            .await
            .map_err(StorageError::from_turso)?;
        let result = async {
            tx.execute(
                sql!(INSERT INTO actors (kind, display_name, created_at) VALUES (?1, ?2, ?3)),
                params_from_iter([ActorKind::User.to_sql(), username.to_sql(), now.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
            let actor_id = ActorId::from(tx.last_insert_rowid());
            tx.execute(
                sql!(
                    INSERT INTO users (actor_id, username, password_hash, password_change_required_at, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                ),
                params_from_iter([
                    actor_id.to_sql(),
                    username.to_sql(),
                    password_hash.to_sql(),
                    password_change_required_at.to_sql(),
                    now.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
            let user_id = UserId::from(tx.last_insert_rowid());
            let actor = Actor {
                id: actor_id,
                kind: ActorKind::User,
                display_name: username.to_owned(),
                created_at: now,
                disabled_at: None,
            };
            let user = User {
                id: user_id,
                actor_id,
                username: username.to_owned(),
                password_hash: password_hash.clone(),
                password_change_required_at,
                created_at: now,
            };
            Ok((actor, user))
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await.map_err(StorageError::from_turso)?;
                Ok(value)
            }
            Err(err) => {
                let _ = tx.rollback().await;
                Err(err)
            }
        }
    }

    /// Atomically creates the first user (and its actor) when no users exist.
    ///
    /// Returns `None` if a user already exists. The count check and inserts run
    /// inside one `BEGIN IMMEDIATE` transaction, so concurrent setup attempts
    /// serialize and only one succeeds.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the connection, transaction, count query, or
    /// either insert fails.
    pub async fn create_initial_user(
        &self,
        username: &str,
        password_hash: &PasswordHash,
        now: Timestamp,
    ) -> Result<Option<(Actor, User)>> {
        let conn = self.connect()?;
        let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate)
            .await
            .map_err(StorageError::from_turso)?;
        let result = async {
            let count: i64 = {
                let mut rows = tx
                    .query(sql!(SELECT COUNT(*) FROM users), ())
                    .await
                    .map_err(StorageError::from_turso)?;
                match rows.next().await.map_err(StorageError::from_turso)? {
                    Some(row) => get(&row, 0)?,
                    None => 0,
                }
            };
            if count > 0 {
                return Ok(None);
            }
            tx.execute(
                sql!(INSERT INTO actors (kind, display_name, created_at) VALUES (?1, ?2, ?3)),
                params_from_iter([ActorKind::User.to_sql(), username.to_sql(), now.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
            let actor_id = ActorId::from(tx.last_insert_rowid());
            tx.execute(
                sql!(
                    INSERT INTO users (actor_id, username, password_hash, password_change_required_at, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                ),
                params_from_iter([
                    actor_id.to_sql(),
                    username.to_sql(),
                    password_hash.to_sql(),
                    None::<Timestamp>.to_sql(),
                    now.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
            let user_id = UserId::from(tx.last_insert_rowid());
            let actor = Actor {
                id: actor_id,
                kind: ActorKind::User,
                display_name: username.to_owned(),
                created_at: now,
                disabled_at: None,
            };
            let user = User {
                id: user_id,
                actor_id,
                username: username.to_owned(),
                password_hash: password_hash.clone(),
                password_change_required_at: None,
                created_at: now,
            };
            Ok(Some((actor, user)))
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await.map_err(StorageError::from_turso)?;
                Ok(value)
            }
            Err(err) => {
                let _ = tx.rollback().await;
                Err(err)
            }
        }
    }
}
