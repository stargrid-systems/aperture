//! Migration 0001: artifact catalog, task invocations, and structured logs.

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

    CREATE TABLE log_spans (
        id INTEGER PRIMARY KEY,
        parent_id INTEGER REFERENCES log_spans (id),
        name TEXT NOT NULL,
        level INTEGER NOT NULL,
        target TEXT NOT NULL,
        file TEXT,
        line INTEGER,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        fields BLOB
    ) STRICT;
    CREATE INDEX idx_log_spans_parent ON log_spans (parent_id);
    CREATE INDEX idx_log_spans_started ON log_spans (started_at);
    CREATE INDEX idx_log_spans_target ON log_spans (target);

    CREATE TABLE log_events (
        id INTEGER PRIMARY KEY,
        span_id INTEGER REFERENCES log_spans (id),
        level INTEGER NOT NULL,
        target TEXT NOT NULL,
        message TEXT,
        timestamp INTEGER NOT NULL,
        file TEXT,
        line INTEGER,
        boot_id TEXT,
        fields BLOB
    ) STRICT;
    CREATE INDEX idx_log_events_timestamp ON log_events (timestamp);
    CREATE INDEX idx_log_events_level ON log_events (level);
    CREATE INDEX idx_log_events_target ON log_events (target);
    CREATE INDEX idx_log_events_span_id ON log_events (span_id);
    CREATE INDEX idx_log_events_boot_id ON log_events (boot_id);
);
