//! Storage layer for the Aperture gateway.
//!
//! A small repository abstraction over an embedded turso (SQLite-compatible)
//! database. Engine details stay inside this crate, so the rest of Aperture
//! depends only on the domain types exposed here. That keeps the engine
//! swappable behind this boundary.

use std::time::Duration;

use turso::{Builder, Connection, Database};

pub use self::artifact::{Artifact, ArtifactKey, ArtifactRepository, VersionSort};
use self::error::database;
pub use self::error::{Result, StorageError};
pub use self::log::{
    BootInfo, Event, EventFilter, EventRecord, Level, LogBatch, LogRepository, Span, SpanFilter,
    SpanParentFilter, SpanRecord,
};
use self::migration::run;
pub use self::page::{ListQuery, Order, Page};
pub use self::task::{
    InvalidJsonPath, JsonField, JsonFilter, JsonPath, ParentFilter, StatusFilter, TaskInvocation,
    TaskRepository, TaskStatus,
};

mod artifact;
mod error;
mod log;
mod macros;
mod migration;
mod page;
mod row;
mod task;

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
            .build()
            .await
            .map_err(database)?;
        let conn = db.connect().map_err(database)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
        run(&conn).await?;
        Ok(Self { db })
    }

    /// Creates a fresh independent connection with the busy timeout applied.
    fn connect(&self) -> Result<Connection> {
        let conn = self.db.connect().map_err(database)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
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

    /// Returns the repository over the structured log tables.
    pub fn logs(&self) -> Result<LogRepository> {
        Ok(LogRepository::new(self.connect()?))
    }
}
