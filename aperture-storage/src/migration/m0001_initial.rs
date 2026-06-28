//! Migration 0001: artifact catalog and download history.

use crate::macros::sql;

pub(super) const SQL: &str = sql!(
    CREATE TABLE artifacts (
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL,
        source TEXT NOT NULL,
        digest TEXT NOT NULL,
        media_type TEXT,
        version TEXT,
        size_bytes INTEGER NOT NULL,
        downloaded_at INTEGER NOT NULL,
        verified_at INTEGER,
        UNIQUE (key, digest)
    );
    CREATE INDEX idx_artifacts_key ON artifacts (key);
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
