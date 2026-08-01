//! Periodic task schedules.

use jiff::Timestamp;
use serde_json::Value;
use turso::{Connection, Row, params_from_iter};

use crate::error::{Result, StorageError};
use crate::interval::Interval;
use crate::macros::{db_id, sql};
use crate::page::{CursorValue, Keyset, ListQuery, Order, Page, Paginator};
use crate::query::{Assignments, Filters};
use crate::sql::{Columns, ToSql};
use crate::task::TaskId;

db_id! {
    /// Primary key of a row in the `task_schedules` table.
    pub struct TaskScheduleId;
}

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

/// Columns selected for a [`TaskSchedule`], in [`TaskSchedule::try_from`]
/// order.
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

/// A periodic task schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSchedule {
    pub id: TaskScheduleId,
    pub kind: String,
    pub input: Value,
    pub interval: Interval,
    pub next_run_at: Timestamp,
    pub last_run_at: Option<Timestamp>,
    pub last_task_id: Option<TaskId>,
    pub enabled: bool,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone)]
pub struct NewTaskSchedule {
    pub kind: String,
    pub input: Value,
    pub interval: Interval,
    pub next_run_at: Timestamp,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Default)]
pub struct TaskSchedulePatch {
    pub interval: Option<Interval>,
    pub enabled: Option<bool>,
}

pub struct TaskScheduleRepository {
    connection: Connection,
}

impl TaskScheduleRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new schedule and returns its assigned id.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    #[tracing::instrument(level = "info", skip(self, new))]
    pub async fn create(&self, new: &NewTaskSchedule) -> Result<TaskScheduleId> {
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
        Ok(TaskScheduleId::from(self.connection.last_insert_rowid()))
    }

    /// Returns the task schedule with `id`, if it exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: TaskScheduleId) -> Result<Option<TaskSchedule>> {
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
            Some(row) => Ok(Some(TaskSchedule::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Oldest first by id.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
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
            items.push(TaskSchedule::try_from(&row)?);
        }
        Ok(paginator.finish(items, |schedule| {
            (CursorValue::Int(schedule.id.get()), schedule.id.get())
        }))
    }

    /// Returns `None` if no schedule has `id`.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the update or follow-up read fails.
    #[tracing::instrument(level = "info", skip(self, patch))]
    pub async fn update(
        &self,
        id: TaskScheduleId,
        patch: &TaskSchedulePatch,
    ) -> Result<Option<TaskSchedule>> {
        let mut assignments = Assignments::new();
        assignments.set_opt(col::INTERVAL_US, patch.interval.as_ref());
        assignments.set_opt(col::ENABLED, patch.enabled.as_ref());
        if assignments.is_empty() {
            return self.get(id).await;
        }
        let set_clause = assignments.set_clause().to_owned();
        let mut params = assignments.into_params();
        params.push(id.to_sql());
        let sql = format!("UPDATE task_schedules SET {set_clause} WHERE id = ?");
        self.connection
            .execute(&sql, params_from_iter(params))
            .await
            .map_err(StorageError::from_turso)?;
        self.get(id).await
    }

    /// Returns whether a row was removed.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the lookup or delete fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete(&self, id: TaskScheduleId) -> Result<bool> {
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

    /// Enabled schedules whose `next_run_at` is at or before `now`, ordered by
    /// `next_run_at` then `id`.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
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
            out.push(TaskSchedule::try_from(&row)?);
        }
        Ok(out)
    }

    /// Advances `next_run_at` by one interval and records the spawn. Pass
    /// `None` for `last_task_id` when the spawn failed.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::InvalidTimestamp` if the next run time overflows
    /// the timestamp range, or `StorageError::Database` if the update fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn mark_run(
        &self,
        id: TaskScheduleId,
        now: Timestamp,
        interval: &Interval,
        last_task_id: Option<TaskId>,
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

impl TryFrom<&Row> for TaskSchedule {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
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
}
