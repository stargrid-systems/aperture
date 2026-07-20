//! Storage layer for the Aperture gateway.
//!
//! A small repository abstraction over an embedded turso (SQLite-compatible)
//! database. Engine details stay inside this crate, so the rest of Aperture
//! depends only on the domain types exposed here. That keeps the engine
//! swappable behind this boundary.

use std::time::Duration;

use turso::{Builder, Connection, Database};

pub use self::artifact::{Artifact, ArtifactKeyEntry, ArtifactRepository, VersionSort};
pub use self::digest::{Digest, DigestAlgorithm, InvalidDigest};
pub use self::error::{Result, StorageError};
pub use self::id::DbId;
pub use self::interval::{Interval, InvalidInterval};
pub use self::key::{ArtifactKey, InvalidArtifactKey, MAX_LEN as ARTIFACT_KEY_MAX_LEN};
pub use self::log::{
    BootInfo, Event, EventFilter, EventRecord, Level, LogBatch, LogRepository, Span, SpanFilter,
    SpanParentFilter, SpanRecord,
};
pub use self::media_type::{InvalidMediaType, MediaType};
pub use self::page::{ListQuery, Order, Page};
pub use self::task::{
    InvalidJsonPath, JsonField, JsonFilter, JsonPath, ParentFilter, StatusFilter, TaskInvocation,
    TaskRepository, TaskStatus,
};
pub use self::task_schedule::{
    NewTaskSchedule, TaskSchedule, TaskSchedulePatch, TaskScheduleRepository,
};

mod artifact;
mod digest;
mod error;
mod id;
mod interval;
mod key;
mod log;
mod macros;
mod media_type;
mod migration;
mod page;
mod query;
mod sql;
mod task;
mod task_schedule;

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
    pub fn artifacts(&self) -> Result<ArtifactRepository> {
        Ok(ArtifactRepository::new(self.connect()?))
    }

    /// Returns the repository over the task catalog.
    pub fn tasks(&self) -> Result<TaskRepository> {
        Ok(TaskRepository::new(self.connect()?))
    }

    pub fn task_schedules(&self) -> Result<TaskScheduleRepository> {
        Ok(TaskScheduleRepository::new(self.connect()?))
    }

    /// Returns the repository over the structured log tables.
    pub fn logs(&self) -> Result<LogRepository> {
        Ok(LogRepository::new(self.connect()?))
    }
}
