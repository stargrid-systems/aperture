//! Task catalog: the durable record of every task invocation.
//!
//! One row per invocation, identified by a surrogate `id`. The row holds the
//! invocation's definition key, its place in any parent/child hierarchy, its
//! lifecycle status, and its JSON input and output objects. The shapes of those
//! objects are opaque here. The task layer owns them and (de)serialization, so
//! storage stays a plain record of what ran.

use std::result::Result as StdResult;

use jiff::Timestamp;
use serde_json::Value;
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::page::{CursorValue, Keyset, ListQuery, Listing, Order, Page, Paginator};
use crate::query::Filters;
use crate::sql::{Columns, ToSql};

db_id! {
    /// Primary key of a row in the `tasks` table.
    pub struct TaskId;
}

mod col {
    pub const CREATED_AT: &str = "created_at";
    pub const ERROR: &str = "error";
    pub const FINISHED_AT: &str = "finished_at";
    pub const ID: &str = "id";
    pub const INITIATOR_ID: &str = "initiator_id";
    pub const INPUT: &str = "input";
    pub const KEY: &str = "key";
    pub const OUTPUT: &str = "output";
    pub const PARENT_ID: &str = "parent_id";
    pub const STARTED_AT: &str = "started_at";
    pub const STATUS: &str = "status";
}

/// Columns selected for a [`TaskInvocation`], in [`TaskInvocation::try_from`]
/// order.
const TASK_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::KEY,
    col::PARENT_ID,
    col::INITIATOR_ID,
    col::STATUS,
    col::INPUT,
    col::OUTPUT,
    col::ERROR,
    col::CREATED_AT,
    col::STARTED_AT,
    col::FINISHED_AT,
]);

/// Lifecycle state of a single task invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Recorded but not yet started.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Succeeded,
    /// Finished with an error.
    Failed,
    /// Stopped on request before finishing.
    Cancelled,
    /// Still running when the process stopped.
    Interrupted,
}

impl TaskStatus {
    /// Statuses of invocations that are not yet finished.
    pub const ACTIVE: [Self; 2] = [Self::Pending, Self::Running];
    /// Statuses of invocations that have reached a terminal state.
    pub const FINISHED: [Self; 4] = [
        Self::Succeeded,
        Self::Failed,
        Self::Cancelled,
        Self::Interrupted,
    ];

    pub(crate) const fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
            other => Err(StorageError::UnknownTaskStatus(other.to_owned())),
        }
    }
}

/// One recorded task invocation, including its assigned id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInvocation {
    /// Store-assigned id.
    pub id: TaskId,
    /// The key of the task's definition.
    pub key: String,
    /// The parent invocation, if this task was spawned by another.
    pub parent_id: Option<TaskId>,
    /// The actor that initiated this task. Child tasks inherit the parent's.
    pub initiator_id: ActorId,
    /// The lifecycle state.
    pub status: TaskStatus,
    /// JSON value passed to the task at spawn.
    pub input: Value,
    /// JSON value returned by the task on success.
    pub output: Option<Value>,
    /// Failure detail, if any.
    pub error: Option<String>,
    /// When the invocation was recorded.
    pub created_at: Timestamp,
    /// When the invocation started running, if it did.
    pub started_at: Option<Timestamp>,
    /// When the invocation finished, if it did.
    pub finished_at: Option<Timestamp>,
}

/// How to filter a task listing by lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    /// Only invocations that are not yet finished.
    Active,
    /// Only invocations that have finished.
    Finished,
    /// Only invocations in one exact status.
    Exact(TaskStatus),
}

/// Which JSON payload of a task a [`JsonFilter`] reaches into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonField {
    /// The task input.
    Input,
    /// The task output.
    Output,
}

impl JsonField {
    /// The column holding this payload. A fixed identifier, safe to
    /// interpolate.
    const fn column(self) -> &'static str {
        match self {
            Self::Input => col::INPUT,
            Self::Output => col::OUTPUT,
        }
    }
}

/// The maximum length of a [`JsonPath`] body.
const MAX_JSON_PATH_LEN: usize = 128;

/// A validated JSON path body, without the leading `$.`.
///
/// Accepts object keys and array indexes joined by dots, for example `key`,
/// `source.reference`, or `items[0].name`. A key is made of ASCII letters,
/// digits, `_`, and `-`. An index is decimal digits in square brackets.
/// Anything else is rejected at construction, so a path `json_extract` would
/// reject never reaches the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonPath<'a>(&'a str);

impl<'a> JsonPath<'a> {
    /// Validates `path` as a JSON path body. Returns [`InvalidJsonPath`] if it
    /// is empty, too long, or not the accepted key-and-index grammar.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidJsonPath`] if `path` is empty, too long, or malformed.
    pub const fn new(path: &'a str) -> StdResult<Self, InvalidJsonPath> {
        if is_valid_json_path(path) {
            Ok(Self(path))
        } else {
            Err(InvalidJsonPath)
        }
    }

    /// The validated path body.
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

/// A string was not a valid [`JsonPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid JSON path")]
pub struct InvalidJsonPath;

const fn is_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

/// Checks the `key ('.' key | '[' digits ']')*` grammar.
const fn is_valid_json_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_JSON_PATH_LEN {
        return false;
    }
    let bytes = path.as_bytes();
    let mut i = 0;
    loop {
        let key_start = i;
        while i < bytes.len() && is_key_byte(bytes[i]) {
            i += 1;
        }
        if i == key_start {
            return false; // empty key: leading, doubled, or trailing dot
        }
        while i < bytes.len() && bytes[i] == b'[' {
            i += 1;
            let digits_start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i == digits_start || i >= bytes.len() || bytes[i] != b']' {
                return false; // empty or unterminated index
            }
            i += 1;
        }
        if i == bytes.len() {
            return true;
        }
        if bytes[i] != b'.' {
            return false;
        }
        i += 1;
    }
}

/// Matches a task whose `field` JSON equals `value` at `path`. The match is
/// textual, so numeric and boolean fields compare by their text form.
#[derive(Debug, Clone, Copy)]
pub struct JsonFilter<'a> {
    /// Which payload to look in.
    pub field: JsonField,
    /// The JSON path body, without the leading `$.`.
    pub path: JsonPath<'a>,
    /// The value the field must equal.
    pub value: &'a str,
}

/// How to filter a task listing by its place in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentFilter {
    /// Only top-level invocations, with no parent.
    Root,
    /// Only the children of one invocation.
    Of(TaskId),
}

/// Repository over the task catalog.
pub struct TaskRepository {
    connection: Connection,
}

impl TaskRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Records a new invocation in the [`TaskStatus::Pending`] state and
    /// returns its assigned id. `input` is the task's input value.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    #[tracing::instrument(level = "info", skip(self, input))]
    pub async fn create(
        &self,
        key: &str,
        parent_id: Option<TaskId>,
        initiator_id: ActorId,
        input: &Value,
        created_at: Timestamp,
    ) -> Result<TaskId> {
        let params = params_from_iter([
            key.to_sql(),
            parent_id.to_sql(),
            initiator_id.to_sql(),
            TaskStatus::Pending.to_sql(),
            input.to_sql(),
            created_at.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO tasks (key, parent_id, initiator_id, status, input, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(TaskId::from(self.connection.last_insert_rowid()))
    }

    /// Records a new invocation already in the [`TaskStatus::Running`] state
    /// and returns its assigned id. Used when a task starts running the
    /// moment it is created, so no observable [`TaskStatus::Pending`] step
    /// exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    #[tracing::instrument(level = "info", skip(self, input))]
    pub async fn create_running(
        &self,
        key: &str,
        parent_id: Option<TaskId>,
        initiator_id: ActorId,
        input: &Value,
        started_at: Timestamp,
    ) -> Result<TaskId> {
        let params = params_from_iter([
            key.to_sql(),
            parent_id.to_sql(),
            initiator_id.to_sql(),
            TaskStatus::Running.to_sql(),
            input.to_sql(),
            started_at.to_sql(),
            started_at.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO tasks (key, parent_id, initiator_id, status, input, created_at, started_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(TaskId::from(self.connection.last_insert_rowid()))
    }

    /// Marks the invocation with `id` as running.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn mark_running(&self, id: TaskId, started_at: Timestamp) -> Result<()> {
        self.connection
            .execute(
                sql!(UPDATE tasks SET status = ?1, started_at = ?2 WHERE id = ?3),
                params_from_iter([
                    TaskStatus::Running.to_sql(),
                    started_at.to_sql(),
                    id.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Records the terminal outcome of the invocation with `id`. `output` is
    /// the task's output value on success, `error` the detail on failure.
    ///
    /// Only an unfinished row is updated. A row that already reached a terminal
    /// state keeps it, so a late interrupt during shutdown cannot clobber a
    /// task that just succeeded.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
    #[tracing::instrument(level = "info", skip(self, output, error))]
    pub async fn finish(
        &self,
        id: TaskId,
        status: TaskStatus,
        finished_at: Timestamp,
        output: Option<&Value>,
        error: Option<&str>,
    ) -> Result<()> {
        self.connection
            .execute(
                sql!(
                    UPDATE tasks
                    SET status = ?1, finished_at = ?2, output = ?3, error = ?4
                    WHERE id = ?5 AND finished_at IS NULL
                ),
                params_from_iter([
                    status.to_sql(),
                    finished_at.to_sql(),
                    output.to_sql(),
                    error.to_sql(),
                    id.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Returns the invocation with `id`, if it exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: TaskId) -> Result<Option<TaskInvocation>> {
        let sql = format!(
            sql!(SELECT {cols} FROM tasks WHERE id = ?1),
            cols = TASK_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(TaskInvocation::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists invocations, newest first, optionally filtered by `status`,
    /// `key`, `parent`, and any number of `json` field matches over the
    /// input/output payloads. All filters combine with `AND`.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
    #[tracing::instrument(level = "info", skip(self, json, query))]
    pub async fn list(
        &self,
        status: Option<StatusFilter>,
        key: Option<&str>,
        parent: Option<ParentFilter>,
        json: &[JsonFilter<'_>],
        query: &ListQuery,
    ) -> Result<Page<TaskInvocation>> {
        let paginator = Paginator::new(query, Order::Desc, Listing::Tasks)?;
        let keyset = Keyset::unique(col::ID, paginator.query_order());

        let mut filters = Filters::new();
        match status {
            Some(StatusFilter::Exact(status)) => filters.eq_text(col::STATUS, status.as_db()),
            Some(StatusFilter::Active) => {
                filters.one_of(col::STATUS, db_values(&TaskStatus::ACTIVE).iter().copied());
            }
            Some(StatusFilter::Finished) => {
                filters.one_of(
                    col::STATUS,
                    db_values(&TaskStatus::FINISHED).iter().copied(),
                );
            }
            None => {}
        }
        filters.eq_text_opt(col::KEY, key);
        match parent {
            Some(ParentFilter::Root) => filters.raw("parent_id IS NULL"),
            Some(ParentFilter::Of(id)) => filters.eq_int(col::PARENT_ID, id.get()),
            None => {}
        }
        for filter in json {
            filters.json_path_eq(filter.field.column(), filter.path.as_str(), filter.value);
        }
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM tasks {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = TASK_COLUMNS,
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
            items.push(TaskInvocation::try_from(&row)?);
        }
        Ok(paginator.finish(items, |task| {
            (
                CursorValue::Int(task.id.get()),
                CursorValue::Int(task.id.get()),
            )
        }))
    }

    /// Lists the children of `parent_id`, oldest first.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn children(&self, parent_id: TaskId) -> Result<Vec<TaskInvocation>> {
        let sql = format!(
            sql!(SELECT {cols} FROM tasks WHERE parent_id = ?1 ORDER BY id),
            cols = TASK_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([parent_id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            tasks.push(TaskInvocation::try_from(&row)?);
        }
        Ok(tasks)
    }

    /// Lists invocations still in an active state. After a clean start these
    /// are leftovers from a process that stopped mid-run.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list_active(&self) -> Result<Vec<TaskInvocation>> {
        let sql = format!(
            sql!(SELECT {cols} FROM tasks WHERE status IN (?1, ?2) ORDER BY id),
            cols = TASK_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                params_from_iter([TaskStatus::Pending.to_sql(), TaskStatus::Running.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            tasks.push(TaskInvocation::try_from(&row)?);
        }
        Ok(tasks)
    }
}

fn db_values(statuses: &[TaskStatus]) -> Vec<&'static str> {
    statuses.iter().map(|status| status.as_db()).collect()
}

impl TryFrom<&Row> for TaskInvocation {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
            id: TASK_COLUMNS.extract(row, col::ID)?,
            key: TASK_COLUMNS.extract(row, col::KEY)?,
            parent_id: TASK_COLUMNS.extract(row, col::PARENT_ID)?,
            initiator_id: TASK_COLUMNS.extract(row, col::INITIATOR_ID)?,
            status: TASK_COLUMNS.extract(row, col::STATUS)?,
            input: TASK_COLUMNS.extract(row, col::INPUT)?,
            output: TASK_COLUMNS.extract(row, col::OUTPUT)?,
            error: TASK_COLUMNS.extract(row, col::ERROR)?,
            created_at: TASK_COLUMNS.extract(row, col::CREATED_AT)?,
            started_at: TASK_COLUMNS.extract(row, col::STARTED_AT)?,
            finished_at: TASK_COLUMNS.extract(row, col::FINISHED_AT)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_path_accepts_keys_and_indexes() {
        for path in [
            "key",
            "source.reference",
            "a_b-c",
            "items[0]",
            "a[0][1]",
            "a[10].b",
        ] {
            assert!(JsonPath::new(path).is_ok(), "{path:?} should be valid");
        }
    }

    #[test]
    fn json_path_rejects_malformed() {
        for path in [
            "", "a..b", ".a", "a.", "a[]", "a[", "a[0", "[0]", "a[b]", "a b", "a;b",
        ] {
            assert!(JsonPath::new(path).is_err(), "{path:?} should be invalid");
        }
    }

    #[test]
    fn json_path_rejects_overlong() {
        let long = "a".repeat(MAX_JSON_PATH_LEN + 1);
        assert!(JsonPath::new(&long).is_err());
    }
}
