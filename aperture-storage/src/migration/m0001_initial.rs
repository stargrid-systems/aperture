//! Migration 0001: artifact catalog, actors, auth, task invocations, and
//! structured logs.
//!
//! `boot_id` is stored as `BLOB` rather than the turso `uuid` custom type
//! because the latter is broken in STRICT tables as of turso 0.6. See
//! <https://github.com/tursodatabase/turso/issues/6221>. The way it's used now should hopefully be compatible with a future fix.

use crate::macros::sql;

pub(super) const SQL: &str = concat!(
    sql!(
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

        CREATE TABLE actors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            display_name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            disabled_at INTEGER
        ) STRICT;
        CREATE INDEX idx_actors_kind ON actors (kind);
    ),
    "INSERT INTO actors (id, kind, display_name, created_at) VALUES (1, 'system', 'system', 0);\n",
    "UPDATE sqlite_sequence SET seq = 1000 WHERE name = 'actors';\n",
    sql!(
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            actor_id INTEGER NOT NULL REFERENCES actors (id),
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            password_change_required_at INTEGER,
            created_at INTEGER NOT NULL
        ) STRICT;

        CREATE TABLE api_keys (
            id INTEGER PRIMARY KEY,
            actor_id INTEGER NOT NULL REFERENCES actors (id),
            name TEXT NOT NULL,
            key_hash BLOB NOT NULL,
            prefix TEXT NOT NULL,
            last_used_at INTEGER,
            created_at INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX idx_api_keys_prefix ON api_keys (prefix);
        CREATE INDEX idx_api_keys_actor ON api_keys (actor_id);

        CREATE TABLE sessions (
            id INTEGER PRIMARY KEY,
            actor_id INTEGER NOT NULL REFERENCES actors (id),
            token_hash BLOB NOT NULL UNIQUE,
            expires_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX idx_sessions_actor ON sessions (actor_id);
        CREATE INDEX idx_sessions_expires ON sessions (expires_at);

        CREATE TABLE casbin_rule (
            id INTEGER PRIMARY KEY,
            ptype TEXT NOT NULL,
            v0 TEXT NOT NULL,
            v1 TEXT NOT NULL,
            v2 TEXT NOT NULL,
            v3 TEXT NOT NULL,
            v4 TEXT NOT NULL,
            v5 TEXT NOT NULL
        ) STRICT;
        CREATE INDEX idx_casbin_rule_ptype ON casbin_rule (ptype);

        CREATE TABLE tasks (
            id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            parent_id INTEGER REFERENCES tasks (id),
            initiator_id INTEGER NOT NULL REFERENCES actors (id),
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
        CREATE INDEX idx_tasks_initiator ON tasks (initiator_id);

        CREATE TABLE log_spans (
            id INTEGER PRIMARY KEY,
            tracing_id INTEGER NOT NULL,
            parent_tracing_id INTEGER,
            boot_id BLOB NOT NULL,
            name TEXT NOT NULL,
            level INTEGER NOT NULL,
            target TEXT NOT NULL,
            file TEXT,
            line INTEGER,
            started_at INTEGER NOT NULL,
            ended_at INTEGER,
            fields jsonb NOT NULL
        ) STRICT;
        CREATE INDEX idx_log_spans_tracing ON log_spans (tracing_id, boot_id);
        CREATE INDEX idx_log_spans_parent_tracing ON log_spans (parent_tracing_id);
        CREATE INDEX idx_log_spans_started ON log_spans (started_at);
        CREATE INDEX idx_log_spans_target ON log_spans (target);

        CREATE TABLE log_events (
            id INTEGER PRIMARY KEY,
            boot_id BLOB NOT NULL,
            span_tracing_id INTEGER,
            level INTEGER NOT NULL,
            target TEXT NOT NULL,
            message TEXT,
            timestamp INTEGER NOT NULL,
            file TEXT,
            line INTEGER,
            fields jsonb NOT NULL
        ) STRICT;
        CREATE INDEX idx_log_events_timestamp ON log_events (timestamp);
        CREATE INDEX idx_log_events_level ON log_events (level);
        CREATE INDEX idx_log_events_target ON log_events (target);
        CREATE INDEX idx_log_events_span_tracing ON log_events (span_tracing_id);
        CREATE INDEX idx_log_events_boot_id ON log_events (boot_id);

        CREATE VIEW log_spans_resolved AS
        SELECT
            child.id,
            parent.id AS parent_id,
            child.name,
            child.level,
            child.target,
            child.file,
            child.line,
            child.started_at,
            child.ended_at,
            json(child.fields) AS fields
        FROM log_spans child
        LEFT JOIN log_spans parent
            ON child.parent_tracing_id = parent.tracing_id
            AND child.boot_id = parent.boot_id;

        CREATE VIEW log_events_resolved AS
        SELECT
            log_events.id,
            span.id AS span_id,
            log_events.level,
            log_events.target,
            log_events.message,
            log_events.timestamp,
            log_events.file,
            log_events.line,
            log_events.boot_id,
            json(log_events.fields) AS fields
        FROM log_events
        LEFT JOIN log_spans span
            ON log_events.span_tracing_id = span.tracing_id
            AND log_events.boot_id = span.boot_id;
    )
);
