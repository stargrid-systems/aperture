//! Storage layer for the Aperture gateway.
//!
//! A small repository abstraction over an embedded turso (SQLite-compatible)
//! database. Engine details stay inside this crate, so the rest of Aperture
//! depends only on the domain types exposed here. That keeps the engine
//! swappable behind this boundary.

use std::time::Duration;

use turso::{Builder, Database};

pub use self::artifact::{
    Artifact, ArtifactKey, ArtifactRepository, Download, DownloadStatus, VersionSort,
};
use self::error::database;
pub use self::error::{Result, StorageError};
pub use self::log::{
    Event, EventFilter, EventInsertBuilder, EventRecord, Level, LogRepository, LogWriter,
    ParentFilter, Span, SpanFilter, SpanInsertBuilder, SpanRecord,
};
use self::migration::run;
pub use self::page::{ListQuery, Order, Page};

/// Busy timeout for write contention between connections. turso uses WAL mode
/// by default, but two writers still need to take turns. Without a timeout
/// the second writer immediately receives "database is locked".
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

mod artifact;
mod error;
mod log;
mod macros;
mod migration;
mod page;

/// Handle to the gateway's persistent storage.
///
/// Each call to [`artifacts`](Self::artifacts), [`logs`](Self::logs), or
/// [`log_writer`](Self::log_writer) opens a fresh [`Connection`] from the
/// underlying [`Database`]. Connections are independent and do not share a
/// concurrency guard, so simultaneous handlers and background tasks cannot
/// conflict with each other.
pub struct Storage {
    database: Database,
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
        let query = db.connect().map_err(database)?;
        query.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
        run(&query).await?;
        let database = db.clone();
        Ok(Self { database })
    }

    /// Opens a new connection and returns the repository over the artifact
    /// catalog. Each call is independent and does not share a concurrency
    /// guard with other connections.
    pub fn artifacts(&self) -> Result<ArtifactRepository> {
        let conn = self.database.connect().map_err(database)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
        Ok(ArtifactRepository::new(conn))
    }

    /// Opens a new connection and returns the repository over the structured
    /// log tables. Each call is independent and does not share a concurrency
    /// guard with other connections.
    pub fn logs(&self) -> Result<LogRepository> {
        let conn = self.database.connect().map_err(database)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
        Ok(LogRepository::new(conn))
    }

    /// Opens a dedicated [`LogWriter`] with its own connection for batch
    /// inserts from a background task. The connection is isolated from the
    /// query connection used by HTTP handlers.
    pub async fn log_writer(&self) -> Result<LogWriter> {
        let conn = self.database.connect().map_err(database)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
        LogWriter::new(conn).await
    }
}