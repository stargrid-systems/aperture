//! Task catalog: the durable record of every task invocation.
//!
//! One row per invocation, identified by a surrogate `id`. The row holds the
//! invocation's kind, its place in any parent/child hierarchy, its lifecycle
//! status, and its JSON-encoded input and output. The input and output payloads
//! are opaque here. The task layer owns their shapes and (de)serialization, so
//! storage stays a plain record of what ran.

use std::result::Result as StdResult;

use jiff::Timestamp;
use turso::{Connection, Row, Value, params_from_iter};

use crate::error::{Result, StorageError, database};
use crate::macros::sql;
use crate::page::{CursorValue, Filters, Keyset, ListQuery, Order, Page, Paginator};
use crate::row::{
    int_or_null, opt_int, opt_text, opt_ts, req_int, req_text, req_ts, text_ref_or_null,
};

/// Columns selected for a [`TaskInvocation`], in [`row_to_task`] order.
const TASK_COLUMNS: &str =
    "id, kind, parent_id, status, input, output, error, created_at, started_at, finished_at";

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

    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
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
#[derive(Debug, Clone, PartialEq)]
pub struct TaskInvocation {
    /// Store-assigned id.
    pub id: i64,
    /// The kind of task, matching a registered definition.
    pub kind: String,
    /// The parent invocation, if this task was spawned by another.
    pub parent_id: Option<i64>,
    /// The lifecycle state.
    pub status: TaskStatus,
    /// JSON-encoded input the task was created with.
    pub input: String,
    /// JSON-encoded output, set once the task succeeds.
    pub output: Option<String>,
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
    fn column(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
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
    pub fn new(path: &'a str) -> StdResult<Self, InvalidJsonPath> {
        if is_valid_json_path(path) {
            Ok(Self(path))
        } else {
            Err(InvalidJsonPath)
        }
    }

    /// The validated path body.
    pub fn as_str(&self) -> &'a str {
        self.0
    }
}

/// A string was not a valid [`JsonPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid JSON path")]
pub struct InvalidJsonPath;

fn is_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

/// Checks the `key ('.' key | '[' digits ']')*` grammar.
fn is_valid_json_path(path: &str) -> bool {
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
    Of(i64),
}

/// Repository over the task catalog.
pub struct TaskRepository {
    connection: Connection,
}

impl TaskRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Records a new invocation in the [`TaskStatus::Pending`] state and
    /// returns its assigned id. `input` is the JSON-encoded task input.
    #[tracing::instrument(level = "info", skip(self, input))]
    pub async fn create(
        &self,
        kind: &str,
        parent_id: Option<i64>,
        input: &str,
        created_at: Timestamp,
    ) -> Result<i64> {
        let params = params_from_iter([
            Value::Text(kind.to_owned()),
            int_or_null(parent_id),
            Value::Text(TaskStatus::Pending.as_db().to_owned()),
            Value::Text(input.to_owned()),
            Value::Integer(created_at.as_millisecond()),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO tasks (kind, parent_id, status, input, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                ),
                params,
            )
            .await
            .map_err(database)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Records a new invocation already in the [`TaskStatus::Running`] state
    /// and returns its assigned id. Used when a task starts running the
    /// moment it is created, so no observable [`TaskStatus::Pending`] step
    /// exists.
    #[tracing::instrument(level = "info", skip(self, input))]
    pub async fn create_running(
        &self,
        kind: &str,
        parent_id: Option<i64>,
        input: &str,
        started_at: Timestamp,
    ) -> Result<i64> {
        let params = params_from_iter([
            Value::Text(kind.to_owned()),
            int_or_null(parent_id),
            Value::Text(TaskStatus::Running.as_db().to_owned()),
            Value::Text(input.to_owned()),
            Value::Integer(started_at.as_millisecond()),
            Value::Integer(started_at.as_millisecond()),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO tasks (kind, parent_id, status, input, created_at, started_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ),
                params,
            )
            .await
            .map_err(database)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Marks the invocation with `id` as running.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn mark_running(&self, id: i64, started_at: Timestamp) -> Result<()> {
        self.connection
            .execute(
                sql!(UPDATE tasks SET status = ?1, started_at = ?2 WHERE id = ?3),
                params_from_iter([
                    Value::Text(TaskStatus::Running.as_db().to_owned()),
                    Value::Integer(started_at.as_millisecond()),
                    Value::Integer(id),
                ]),
            )
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Records the terminal outcome of the invocation with `id`. `output` is
    /// the JSON-encoded result on success, `error` the detail on failure.
    ///
    /// Only an unfinished row is updated. A row that already reached a terminal
    /// state keeps it, so a late interrupt during shutdown cannot clobber a
    /// task that just succeeded.
    #[tracing::instrument(level = "info", skip(self, output, error))]
    pub async fn finish(
        &self,
        id: i64,
        status: TaskStatus,
        finished_at: Timestamp,
        output: Option<&str>,
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
                    Value::Text(status.as_db().to_owned()),
                    Value::Integer(finished_at.as_millisecond()),
                    text_ref_or_null(output),
                    text_ref_or_null(error),
                    Value::Integer(id),
                ]),
            )
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Returns the invocation with `id`, if it exists.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: i64) -> Result<Option<TaskInvocation>> {
        let sql = format!(
            sql!(SELECT {cols} FROM tasks WHERE id = ?1),
            cols = TASK_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(id)]))
            .await
            .map_err(database)?;
        match rows.next().await.map_err(database)? {
            Some(row) => Ok(Some(row_to_task(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists invocations, newest first, optionally filtered by `status`,
    /// `kind`, `parent`, and any number of `json` field matches over the
    /// input/output payloads. All filters combine with `AND`.
    #[tracing::instrument(level = "info", skip(self, json, query))]
    pub async fn list(
        &self,
        status: Option<StatusFilter>,
        kind: Option<&str>,
        parent: Option<ParentFilter>,
        json: &[JsonFilter<'_>],
        query: &ListQuery,
    ) -> Result<Page<TaskInvocation>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::unique("id", paginator.query_order());

        let mut filters = Filters::new();
        match status {
            Some(StatusFilter::Exact(status)) => filters.eq_text("status", Some(status.as_db())),
            Some(StatusFilter::Active) => filters.one_of("status", &db_values(&TaskStatus::ACTIVE)),
            Some(StatusFilter::Finished) => {
                filters.one_of("status", &db_values(&TaskStatus::FINISHED));
            }
            None => {}
        }
        filters.eq_text("kind", kind);
        match parent {
            Some(ParentFilter::Root) => filters.raw("parent_id IS NULL"),
            Some(ParentFilter::Of(id)) => filters.eq_int("parent_id", Some(id)),
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
            .map_err(database)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            items.push(row_to_task(&row)?);
        }
        Ok(paginator.finish(items, |task| (CursorValue::Int(task.id), task.id)))
    }

    /// Lists the children of `parent_id`, oldest first.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn children(&self, parent_id: i64) -> Result<Vec<TaskInvocation>> {
        let sql = format!(
            sql!(SELECT {cols} FROM tasks WHERE parent_id = ?1 ORDER BY id),
            cols = TASK_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Integer(parent_id)]))
            .await
            .map_err(database)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            tasks.push(row_to_task(&row)?);
        }
        Ok(tasks)
    }

    /// Lists invocations still in an active state. After a clean start these
    /// are leftovers from a process that stopped mid-run.
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
                params_from_iter([
                    Value::Text(TaskStatus::Pending.as_db().to_owned()),
                    Value::Text(TaskStatus::Running.as_db().to_owned()),
                ]),
            )
            .await
            .map_err(database)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            tasks.push(row_to_task(&row)?);
        }
        Ok(tasks)
    }
}

fn db_values(statuses: &[TaskStatus]) -> Vec<&'static str> {
    statuses.iter().map(|status| status.as_db()).collect()
}

fn row_to_task(row: &Row) -> Result<TaskInvocation> {
    Ok(TaskInvocation {
        id: req_int(row, 0)?,
        kind: req_text(row, 1)?,
        parent_id: opt_int(row, 2)?,
        status: TaskStatus::from_db(&req_text(row, 3)?)?,
        input: req_text(row, 4)?,
        output: opt_text(row, 5)?,
        error: opt_text(row, 6)?,
        created_at: req_ts(row, 7)?,
        started_at: opt_ts(row, 8)?,
        finished_at: opt_ts(row, 9)?,
    })
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
