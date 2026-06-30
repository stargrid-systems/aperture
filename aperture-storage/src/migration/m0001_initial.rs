//! Migration 0001: artifact catalog, download history, and structured logs.

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

    CREATE TABLE spans (
        id INTEGER PRIMARY KEY,
        parent_id INTEGER REFERENCES spans(id),
        name TEXT NOT NULL,
        level TEXT NOT NULL,
        target TEXT NOT NULL,
        file TEXT,
        line INTEGER,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        fields TEXT
    );
    CREATE INDEX idx_spans_parent ON spans (parent_id);
    CREATE INDEX idx_spans_started ON spans (started_at);
    CREATE INDEX idx_spans_target ON spans (target);

    CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        span_id INTEGER REFERENCES spans(id),
        level TEXT NOT NULL,
        target TEXT NOT NULL,
        message TEXT,
        timestamp INTEGER NOT NULL,
        file TEXT,
        line INTEGER,
        fields TEXT
    );
    CREATE INDEX idx_events_timestamp ON events (timestamp);
    CREATE INDEX idx_events_level ON events (level);
    CREATE INDEX idx_events_target ON events (target);
    CREATE INDEX idx_events_span_id ON events (span_id);
    CREATE INDEX idx_events_message_fts ON events USING fts (message);
);
