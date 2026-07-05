//! Storage layer for the Aperture gateway.
//!
//! A small repository abstraction over an embedded turso (SQLite-compatible)
//! database. Engine details stay inside this crate, so the rest of Aperture
//! depends only on the domain types exposed here. That keeps the engine
//! swappable behind this boundary.

use std::time::Duration;

use turso::{Builder, Connection};

pub use self::artifact::{Artifact, ArtifactKey, ArtifactRepository, VersionSort};
use self::error::database;
pub use self::error::{Result, StorageError};
pub use self::log::{
    BootInfo, Event, EventFilter, EventInsertBuilder, EventRecord, Level, LogRepository, LogWriter,
    Span, SpanFilter, SpanInsertBuilder, SpanParentFilter, SpanRecord,
};
use self::migration::run;
pub use self::page::{ListQuery, Order, Page};
pub use self::task::{
    InvalidJsonPath, JsonField, JsonFilter, JsonPath, ParentFilter, StatusFilter, TaskInvocation,
    TaskRepository, TaskStatus,
};

/// Busy timeout for write contention. turso uses WAL mode by default, but two
/// writers still need to take turns. Without a timeout the second writer
/// immediately receives "database is locked".
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

mod artifact;
mod error;
mod log;
mod macros;
mod migration;
mod page;
mod row;
mod task;

/// Handle to the gateway's persistent storage.
#[derive(Clone)]
pub struct Storage {
    connection: Connection,
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
        let connection = db.connect().map_err(database)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
        run(&connection).await?;
        Ok(Self { connection })
    }

    /// Returns the repository over the artifact catalog.
    pub fn artifacts(&self) -> ArtifactRepository {
        ArtifactRepository::new(self.connection.clone())
    }

    /// Returns the repository over the task catalog.
    pub fn tasks(&self) -> TaskRepository {
        TaskRepository::new(self.connection.clone())
    }

    /// Returns the repository over the structured log tables.
    pub fn logs(&self) -> LogRepository {
        LogRepository::new(self.connection.clone())
    }

    /// Returns a [`LogWriter`] for batch inserts from a background task.
    pub async fn log_writer(&self) -> Result<LogWriter> {
        LogWriter::new(self.connection.clone()).await
    }
}
