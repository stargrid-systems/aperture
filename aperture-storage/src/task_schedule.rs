//! Periodic task schedules.
//!
//! A task schedule row describes a task kind and JSON input that the scheduler
//! should re-run on a fixed cadence. The scheduler advances `next_run_at`
//! after each spawn. Schedules are exposed through the HTTP API so operators
//! can list, create, and disable them.

use jiff::Timestamp;
use serde_json::Value;
use turso::{Connection, Row, params_from_iter};

use crate::error::{Result, StorageError};
use crate::id::DbId;
use crate::interval::Interval;
use crate::macros::sql;
use crate::page::{CursorValue, Keyset, ListQuery, Order, Page, Paginator};
use crate::query::Filters;
use crate::sql::{Columns, ToSql};

mod col {
    pub const CREATED_AT: &str = "created_at";
    pub const ENABLED: &str = "enabled";
    pub const ID: &str = "id";
    pub const INPUT: &str = "input";
    pub const INTERVAL_US: &str = "interval_us";
    pub const KIND: &str = "kind";
    pub const LAST_RUN_AT: &str = "last_run_at";
    pub const LAST_TASK_ID: &str = "last_task_id";
    pub const NEXT_RUN_AT: &str = "next_run_at";
}

/// Columns selected for a [`TaskSchedule`], in [`row_to_schedule`] order.
const SCHEDULE_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::KIND,
    col::INPUT,
    col::INTERVAL_US,
    col::NEXT_RUN_AT,
    col::LAST_RUN_AT,
    col::LAST_TASK_ID,
    col::ENABLED,
    col::CREATED_AT,
]);

/// A periodic task schedule. The scheduler spawns `kind` with `input` every
/// `interval`, advancing `next_run_at` after each spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSchedule {
    /// Store-assigned id.
    pub id: DbId,
    /// The kind of task to spawn, matching a registered definition.
    pub kind: String,
    /// JSON value passed to each spawned invocation.
    pub input: Value,
    /// Spawn cadence.
    pub interval: Interval,
    /// When the next spawn is due.
    pub next_run_at: Timestamp,
    /// When the most recent spawn fired, if any.
    pub last_run_at: Option<Timestamp>,
    /// The id of the most recent spawned invocation, if any.
    pub last_task_id: Option<DbId>,
    /// Whether the scheduler should fire this schedule.
    pub enabled: bool,
    /// When the schedule was created.
    pub created_at: Timestamp,
}

/// Payload for creating a new task schedule.
#[derive(Debug, Clone)]
pub struct NewTaskSchedule {
    pub kind: String,
    pub input: Value,
    pub interval: Interval,
    pub next_run_at: Timestamp,
    pub created_at: Timestamp,
}

/// Payload for patching an existing task schedule. `None` fields are left
/// alone.
#[derive(Debug, Clone, Default)]
pub struct TaskSchedulePatch {
    pub interval: Option<Interval>,
    pub enabled: Option<bool>,
}

/// Repository over the task schedule catalog.
pub struct TaskScheduleRepository {
    connection: Connection,
}

impl TaskScheduleRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Records a new task schedule and returns its assigned id.
    #[tracing::instrument(level = "info", skip(self, new))]
    pub async fn create(&self, new: &NewTaskSchedule) -> Result<DbId> {
        let params = params_from_iter([
            new.kind.to_sql(),
            new.input.to_sql(),
            new.interval.to_sql(),
            new.next_run_at.to_sql(),
            new.created_at.to_sql(),
            true.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO task_schedules
                        (kind, input, interval_us, next_run_at, created_at, enabled)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(DbId::from(self.connection.last_insert_rowid()))
    }

    /// Returns the task schedule with `id`, if it exists.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: DbId) -> Result<Option<TaskSchedule>> {
        let sql = format!(
            sql!(SELECT {cols} FROM task_schedules WHERE id = ?1),
            cols = SCHEDULE_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_schedule(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists task schedules, oldest first by id.
    #[tracing::instrument(level = "info", skip(self, query))]
    pub async fn list(&self, query: &ListQuery) -> Result<Page<TaskSchedule>> {
        let paginator = Paginator::new(query, Order::Asc)?;
        let keyset = Keyset::unique(col::ID, paginator.query_order());

        let mut filters = Filters::new();
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM task_schedules {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = SCHEDULE_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            items.push(row_to_schedule(&row)?);
        }
        Ok(paginator.finish(items, |schedule| {
            (CursorValue::Int(schedule.id.get()), schedule.id.get())
        }))
    }

    /// Applies `patch` to the task schedule with `id`. Returns the updated row,
    /// or `None` if no schedule has that id.
    #[tracing::instrument(level = "info", skip(self, patch))]
    pub async fn update(
        &self,
        id: DbId,
        patch: &TaskSchedulePatch,
    ) -> Result<Option<TaskSchedule>> {
        let mut sets: Vec<&'static str> = Vec::new();
        if patch.interval.is_some() {
            sets.push("interval_us = ?");
        }
        if patch.enabled.is_some() {
            sets.push("enabled = ?");
        }
        if sets.is_empty() {
            return self.get(id).await;
        }
        let set_clause = sets.join(", ");
        let sql = format!("UPDATE task_schedules SET {set_clause} WHERE id = ?");
        let mut params: Vec<turso::Value> = Vec::new();
        if let Some(interval) = &patch.interval {
            params.push(interval.to_sql());
        }
        if let Some(enabled) = patch.enabled {
            params.push(enabled.to_sql());
        }
        params.push(id.to_sql());
        self.connection
            .execute(&sql, params_from_iter(params))
            .await
            .map_err(StorageError::from_turso)?;
        self.get(id).await
    }

    /// Deletes the task schedule with `id`. Returns whether a row was removed.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete(&self, id: DbId) -> Result<bool> {
        let existed = self.get(id).await?.is_some();
        if existed {
            self.connection
                .execute(
                    sql!(DELETE FROM task_schedules WHERE id = ?1),
                    params_from_iter([id.to_sql()]),
                )
                .await
                .map_err(StorageError::from_turso)?;
        }
        Ok(existed)
    }

    /// Returns enabled schedules whose `next_run_at` is at or before `now`,
    /// ordered by `next_run_at` then `id`. `limit` caps the batch size so the
    /// scheduler cannot pin a tick on a runaway backlog.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_due(&self, now: Timestamp, limit: usize) -> Result<Vec<TaskSchedule>> {
        let sql = format!(
            sql!(
                SELECT {cols} FROM task_schedules
                WHERE enabled = TRUE AND next_run_at <= ?1
                ORDER BY next_run_at, id LIMIT ?2
            ),
            cols = SCHEDULE_COLUMNS
        );
        let limit_int: i64 = limit.try_into().unwrap_or(i64::MAX);
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([now.to_sql(), limit_int.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            out.push(row_to_schedule(&row)?);
        }
        Ok(out)
    }

    /// Records that the schedule with `id` fired at `now`, advancing
    /// `next_run_at` by one interval and pointing `last_task_id` at the
    /// invocation that was spawned. Pass `None` for `last_task_id` when the
    /// spawn failed so the column stays NULL.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn mark_run(
        &self,
        id: DbId,
        now: Timestamp,
        interval: &Interval,
        last_task_id: Option<DbId>,
    ) -> Result<()> {
        // saturating_add turns overflow into i64::MAX, which is outside jiff's
        // timestamp range, so from_microsecond reports InvalidTimestamp.
        let next_micros = now.as_microsecond().saturating_add(interval.as_micros());
        let next_run_at = Timestamp::from_microsecond(next_micros).map_err(|_| {
            StorageError::InvalidTimestamp {
                micros: next_micros,
            }
        })?;
        self.connection
            .execute(
                sql!(
                    UPDATE task_schedules
                    SET last_run_at = ?1, last_task_id = ?2, next_run_at = ?3
                    WHERE id = ?4
                ),
                params_from_iter([
                    now.to_sql(),
                    last_task_id.to_sql(),
                    next_run_at.to_sql(),
                    id.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }
}

fn row_to_schedule(row: &Row) -> Result<TaskSchedule> {
    Ok(TaskSchedule {
        id: SCHEDULE_COLUMNS.extract(row, col::ID)?,
        kind: SCHEDULE_COLUMNS.extract(row, col::KIND)?,
        input: SCHEDULE_COLUMNS.extract(row, col::INPUT)?,
        interval: SCHEDULE_COLUMNS.extract(row, col::INTERVAL_US)?,
        next_run_at: SCHEDULE_COLUMNS.extract(row, col::NEXT_RUN_AT)?,
        last_run_at: SCHEDULE_COLUMNS.extract(row, col::LAST_RUN_AT)?,
        last_task_id: SCHEDULE_COLUMNS.extract(row, col::LAST_TASK_ID)?,
        enabled: SCHEDULE_COLUMNS.extract(row, col::ENABLED)?,
        created_at: SCHEDULE_COLUMNS.extract(row, col::CREATED_AT)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(micros: i64) -> Timestamp {
        Timestamp::from_microsecond(micros).unwrap()
    }

    fn interval(micros: i64) -> Interval {
        Interval::from_micros(micros).unwrap()
    }

    fn new_schedule(kind: &str, interval_micros: i64, next_run_at: i64) -> NewTaskSchedule {
        NewTaskSchedule {
            kind: kind.to_owned(),
            input: Value::Object(serde_json::Map::new()),
            interval: interval(interval_micros),
            next_run_at: ts(next_run_at),
            created_at: ts(0),
        }
    }

    #[tokio::test]
    async fn create_get_list_update_delete() {
        let storage = crate::Storage::open(":memory:").await.unwrap();
        let repo = storage.task_schedules().unwrap();

        let id = repo
            .create(&new_schedule("rotate-certificate", 86_400_000_000, 1_000))
            .await
            .unwrap();
        let fetched = repo.get(id).await.unwrap().unwrap();
        assert_eq!(fetched.kind, "rotate-certificate");
        assert_eq!(fetched.interval, interval(86_400_000_000));
        assert_eq!(fetched.next_run_at, ts(1_000));
        assert!(fetched.enabled);

        let page = repo.list(&ListQuery::default()).await.unwrap();
        assert_eq!(page.items.len(), 1);

        let updated = repo
            .update(
                id,
                &TaskSchedulePatch {
                    enabled: Some(false),
                    interval: Some(interval(60_000_000)),
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert!(!updated.enabled);
        assert_eq!(updated.interval, interval(60_000_000));

        assert!(repo.delete(id).await.unwrap());
        assert!(repo.get(id).await.unwrap().is_none());
        assert!(!repo.delete(id).await.unwrap());
    }

    #[tokio::test]
    async fn list_due_returns_only_enabled_past_due() {
        let storage = crate::Storage::open(":memory:").await.unwrap();
        let repo = storage.task_schedules().unwrap();
        // Due, enabled.
        repo.create(&new_schedule("a", 1_000, 500)).await.unwrap();
        // Not yet due.
        repo.create(&new_schedule("b", 1_000, 5_000)).await.unwrap();
        // Due but disabled.
        let disabled_id = repo.create(&new_schedule("c", 1_000, 500)).await.unwrap();
        repo.update(
            disabled_id,
            &TaskSchedulePatch {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let due = repo.list_due(ts(1_000), 16).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].kind, "a");
    }

    #[tokio::test]
    async fn mark_run_advances_next_run_at() {
        let storage = crate::Storage::open(":memory:").await.unwrap();
        let repo = storage.task_schedules().unwrap();
        let id = repo
            .create(&new_schedule("a", 60_000_000, 1_000))
            .await
            .unwrap();
        repo.mark_run(id, ts(2_000), &interval(60_000_000), Some(DbId::from(42)))
            .await
            .unwrap();
        let fetched = repo.get(id).await.unwrap().unwrap();
        assert_eq!(fetched.last_run_at, Some(ts(2_000)));
        assert_eq!(fetched.last_task_id, Some(DbId::from(42)));
        assert_eq!(fetched.next_run_at, ts(60_002_000));
    }

    #[tokio::test]
    async fn mark_run_stores_null_last_task_id_on_failure() {
        let storage = crate::Storage::open(":memory:").await.unwrap();
        let repo = storage.task_schedules().unwrap();
        let id = repo
            .create(&new_schedule("a", 60_000_000, 1_000))
            .await
            .unwrap();
        repo.mark_run(id, ts(2_000), &interval(60_000_000), None)
            .await
            .unwrap();
        let fetched = repo.get(id).await.unwrap().unwrap();
        assert_eq!(fetched.last_task_id, None);
        assert_eq!(fetched.next_run_at, ts(60_002_000));
    }
}
