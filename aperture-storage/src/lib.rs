//! Storage layer for the Aperture gateway.
//!
//! A small repository abstraction over an embedded turso (SQLite-compatible)
//! database. Engine details stay inside this crate, so the rest of Aperture
//! depends only on the domain types exposed here. That keeps the engine
//! swappable behind this boundary.

use turso::{Builder, Connection};

pub use self::artifact::{Artifact, ArtifactKey, ArtifactRepository, VersionSort};
use self::error::database;
pub use self::error::{Result, StorageError};
use self::migration::run;
pub use self::page::{ListQuery, Order, Page};
pub use self::task::{ParentFilter, StatusFilter, TaskInvocation, TaskRepository, TaskStatus};

mod artifact;
mod error;
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
        let db = Builder::new_local(path).build().await.map_err(database)?;
        let connection = db.connect().map_err(database)?;
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
}
