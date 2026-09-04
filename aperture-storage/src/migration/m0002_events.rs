//! Migration 0002: persisted domain events.

use crate::macros::sql;

const TABLES: &str = sql!(
    CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL,
        data jsonb NOT NULL,
        actor INTEGER NOT NULL REFERENCES actors (id),
        timestamp timestamp_us NOT NULL
    ) STRICT;
    CREATE INDEX idx_events_key ON events (key);
    CREATE INDEX idx_events_timestamp ON events (timestamp);
);

pub(super) const STATEMENTS: &[&str] = &[TABLES];
