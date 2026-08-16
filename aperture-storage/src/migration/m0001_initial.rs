//! Migration 0001: artifact catalog, actors, auth, task invocations, task
//! schedules, structured logs, settings, and events.

use crate::macros::sql;

const CUSTOM_TYPES: &str = "\
CREATE TYPE timestamp_us BASE INTEGER
    ENCODE value
    DECODE value
    OPERATOR '<'
    OPERATOR '=';
CREATE TYPE duration_us BASE INTEGER
    ENCODE CASE WHEN value > 0 THEN value ELSE RAISE(ABORT, 'duration must be positive') END
    DECODE value
    OPERATOR '<'
    OPERATOR '=';
";

const TABLES: &str = sql!(
    CREATE TABLE artifacts (
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL,
        source TEXT NOT NULL,
        digest TEXT NOT NULL,
        media_type TEXT,
        version TEXT,
        size_bytes INTEGER NOT NULL CHECK (size_bytes > 0),
        downloaded_at timestamp_us NOT NULL,
        verified_at timestamp_us,
        UNIQUE (key, digest)
    ) STRICT;
    CREATE INDEX idx_artifacts_key ON artifacts (key);

    CREATE TABLE actors (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        kind TEXT NOT NULL,
        display_name TEXT NOT NULL,
        created_at timestamp_us NOT NULL,
        disabled_at timestamp_us
    ) STRICT;
    CREATE INDEX idx_actors_kind ON actors (kind);

    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        actor_id INTEGER NOT NULL REFERENCES actors (id),
        username TEXT NOT NULL UNIQUE,
        password_hash TEXT NOT NULL,
        password_change_required_at timestamp_us,
        created_at timestamp_us NOT NULL
    ) STRICT;

    CREATE TABLE api_keys (
        id INTEGER PRIMARY KEY,
        actor_id INTEGER NOT NULL REFERENCES actors (id),
        name TEXT NOT NULL,
        key_hash BLOB NOT NULL,
        prefix TEXT NOT NULL UNIQUE,
        last_used_at timestamp_us,
        created_at timestamp_us NOT NULL
    ) STRICT;
    CREATE INDEX idx_api_keys_actor ON api_keys (actor_id);

    CREATE TABLE sessions (
        id INTEGER PRIMARY KEY,
        actor_id INTEGER NOT NULL REFERENCES actors (id),
        token_hash BLOB NOT NULL UNIQUE,
        expires_at timestamp_us NOT NULL,
        created_at timestamp_us NOT NULL
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
        key TEXT NOT NULL,
        parent_id INTEGER REFERENCES tasks (id),
        initiator_id INTEGER NOT NULL REFERENCES actors (id),
        status TEXT NOT NULL,
        input jsonb NOT NULL,
        output jsonb,
        error TEXT,
        created_at timestamp_us NOT NULL,
        started_at timestamp_us,
        finished_at timestamp_us
    ) STRICT;
    CREATE INDEX idx_tasks_key ON tasks ("key");
    CREATE INDEX idx_tasks_status ON tasks (status);
    CREATE INDEX idx_tasks_parent ON tasks (parent_id);
    CREATE INDEX idx_tasks_initiator ON tasks (initiator_id);

    CREATE TABLE task_schedules (
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL,
        input jsonb NOT NULL,
        interval_us duration_us NOT NULL,
        next_run_at timestamp_us NOT NULL,
        last_run_at timestamp_us,
        last_task_id INTEGER REFERENCES tasks (id),
        enabled boolean NOT NULL DEFAULT TRUE,
        created_at timestamp_us NOT NULL
    ) STRICT;
    CREATE INDEX idx_task_schedules_key ON task_schedules ("key");
    CREATE INDEX idx_task_schedules_next_run ON task_schedules (next_run_at) WHERE enabled = TRUE;

    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value jsonb NOT NULL,
        updated_at timestamp_us NOT NULL,
        updated_by INTEGER NOT NULL REFERENCES actors (id)
    ) STRICT;

    CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL,
        data jsonb NOT NULL,
        actor INTEGER NOT NULL REFERENCES actors (id),
        timestamp timestamp_us NOT NULL
    ) STRICT;
    CREATE INDEX idx_events_key ON events (key);
    CREATE INDEX idx_events_timestamp ON events (timestamp);

    CREATE TABLE log_spans (
        id INTEGER PRIMARY KEY,
        tracing_id INTEGER NOT NULL,
        parent_tracing_id INTEGER,
        boot_id uuid NOT NULL,
        name TEXT NOT NULL,
        level INTEGER NOT NULL CHECK (level BETWEEN 0 AND 4),
        target TEXT NOT NULL,
        file TEXT,
        line smallint CHECK (line IS NULL OR CAST(line AS INTEGER) > 0),
        started_at timestamp_us NOT NULL,
        ended_at timestamp_us,
        fields jsonb NOT NULL
    ) STRICT;
    CREATE INDEX idx_log_spans_tracing ON log_spans (tracing_id, boot_id);
    CREATE INDEX idx_log_spans_parent_tracing ON log_spans (parent_tracing_id);
    CREATE INDEX idx_log_spans_started ON log_spans (started_at);
    CREATE INDEX idx_log_spans_target ON log_spans (target);

    CREATE TABLE log_events (
        id INTEGER PRIMARY KEY,
        boot_id uuid NOT NULL,
        span_tracing_id INTEGER,
        level INTEGER NOT NULL CHECK (level BETWEEN 0 AND 4),
        target TEXT NOT NULL,
        message TEXT,
        timestamp timestamp_us NOT NULL,
        file TEXT,
        line smallint CHECK (line IS NULL OR CAST(line AS INTEGER) > 0),
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
        child.fields
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
        log_events.fields
    FROM log_events
    LEFT JOIN log_spans span
        ON log_events.span_tracing_id = span.tracing_id
        AND log_events.boot_id = span.boot_id;
);

/// Seeds the reserved system actor (id 1) and reserves the first 1000 ids.
/// Kept out of [`TABLES`] because single-quoted SQL strings inside the `sql!`
/// macro are parsed as Rust char literals.
const ACTOR_SEED: &str = concat!(
    "INSERT INTO actors (id, kind, display_name, created_at) ",
    "VALUES (1, 'system', 'system', 0); ",
    "UPDATE sqlite_sequence SET seq = 1000 WHERE name = 'actors';",
);

pub(super) const STATEMENTS: &[&str] = &[CUSTOM_TYPES, TABLES, ACTOR_SEED];
