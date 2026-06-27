//! Migration 0001: artifact catalog and download history.

use crate::macros::sql;

pub(super) const SQL: &str = sql!(
    CREATE TABLE artifacts (
        name TEXT PRIMARY KEY NOT NULL,
        kind TEXT NOT NULL,
        source TEXT NOT NULL,
        digest TEXT,
        media_type TEXT,
        version TEXT,
        size_bytes INTEGER,
        status TEXT NOT NULL,
        downloaded_at INTEGER,
        verified_at INTEGER
    );
    CREATE TABLE artifact_downloads (
        id INTEGER PRIMARY KEY,
        artifact TEXT NOT NULL,
        started_at INTEGER NOT NULL,
        finished_at INTEGER,
        status TEXT NOT NULL,
        digest TEXT,
        size_bytes INTEGER,
        source TEXT NOT NULL,
        error TEXT
    );
    CREATE INDEX idx_artifact_downloads_artifact ON artifact_downloads (artifact);
);
