//! Storage layer for the Aperture gateway.
//!
//! A small repository abstraction over an embedded turso (SQLite-compatible)
//! database. Engine details stay inside this crate, so the rest of Aperture
//! depends only on the domain types exposed here. That keeps the engine
//! swappable behind this boundary.

use std::time::Duration;

use turso::{Builder, Connection, Database};

pub use self::artifact::{
    Artifact, ArtifactKey, ArtifactRepository, Download, DownloadStatus, VersionSort,
};
pub use self::error::{Result, StorageError};
pub use self::log::{
    Event, EventFilter, EventInsertBuilder, EventRecord, Level, LogRepository, LogWriter, Span,
    SpanFilter, SpanInsertBuilder, SpanRecord,
};
use self::error::database;
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
/// Clones share the same underlying [`Database`]. Each call to
/// [`log_writer`](Self::log_writer) opens a new [`Connection`] with an
/// independent concurrency guard, so a background writer task cannot conflict
/// with the query connection used by HTTP handlers.
pub struct Storage {
    database: Database,
    /// The shared query connection. Cheap to clone; all clones share one
    /// concurrency guard.
    query: Connection,
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
        Ok(Self { database, query })
    }

    /// Returns the repository over the artifact catalog.
    pub fn artifacts(&self) -> ArtifactRepository {
        ArtifactRepository::new(self.query.clone())
    }

    /// Returns the repository over the structured log tables (for queries).
    pub fn logs(&self) -> LogRepository {
        LogRepository::new(self.query.clone())
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