//! Migration 0001: artifact catalog and task invocations.

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
    ) STRICT;
    CREATE INDEX idx_artifacts_key ON artifacts (key);
    CREATE TABLE tasks (
        id INTEGER PRIMARY KEY,
        kind TEXT NOT NULL,
        parent_id INTEGER REFERENCES tasks (id),
        status TEXT NOT NULL,
        input TEXT NOT NULL,
        output TEXT,
        error TEXT,
        created_at INTEGER NOT NULL,
        started_at INTEGER,
        finished_at INTEGER
    ) STRICT;
    CREATE INDEX idx_tasks_kind ON tasks (kind);
    CREATE INDEX idx_tasks_status ON tasks (status);
    CREATE INDEX idx_tasks_parent ON tasks (parent_id);
);
