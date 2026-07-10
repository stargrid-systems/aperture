//! Traits for converting between domain types and turso [`Value`]s.
//!
//! Every type that goes into or comes out of the database implements [`ToSql`]
//! and/or [`FromSql`]. The blanket impl for [`Option<T>`] handles NULL
//! automatically: [`Option<T>`] maps to `NULL` when `None`, and [`ToSql`] or
//! [`FromSql`] on `T` when `Some`.

use jiff::Timestamp;
use serde_json::Map;
use turso::{Row, Value};
use uuid::Uuid;

use crate::error::{Result, StorageError, database};
use crate::id::DbId;
use crate::log::Level;
use crate::task::TaskStatus;

mod columns;
mod domain;
mod integer;
mod json;
mod text;

/// Convert a value to a database [`Value`] for binding.
pub(crate) trait ToSql {
    fn to_sql(&self) -> Value;
}

/// Convert a database [`Value`] back to a domain type.
///
/// `idx` is the column index, used for error messages.
pub(crate) trait FromSql: Sized {
    fn from_sql(value: Value, idx: usize) -> Result<Self>;
}

/// Extracts a value at `idx` from `row`. Shortcut for
/// `T::from_sql(row.get_value(idx)?, idx)`.
pub(crate) fn get<T: FromSql>(row: &Row, idx: usize) -> Result<T> {
    let value = row.get_value(idx).map_err(database)?;
    T::from_sql(value, idx)
}

impl<T: ToSql + ?Sized> ToSql for &T {
    fn to_sql(&self) -> Value {
        (**self).to_sql()
    }
}

impl<T: ToSql> ToSql for Option<T> {
    fn to_sql(&self) -> Value {
        match self {
            Some(v) => v.to_sql(),
            None => Value::Null,
        }
    }
}

impl<T: FromSql> FromSql for Option<T> {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Null => Ok(None),
            other => T::from_sql(other, idx).map(Some),
        }
    }
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

impl ToSql for str {
    fn to_sql(&self) -> Value {
        Value::Text(self.to_owned())
    }
}

impl ToSql for String {
    fn to_sql(&self) -> Value {
        Value::Text(self.clone())
    }
}

impl FromSql for String {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Text(s) => Ok(s),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "text",
                actual,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Integers
// ---------------------------------------------------------------------------

impl ToSql for i64 {
    fn to_sql(&self) -> Value {
        Value::Integer(*self)
    }
}

impl FromSql for i64 {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Integer(v) => Ok(v),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "integer",
                actual,
            }),
        }
    }
}

/// Bitwise cast from `u64` to `i64` for database storage.
///
/// SQLite stores integers as signed 64-bit. Values above `i64::MAX` wrap to
/// negative numbers. Equality comparisons are preserved, but ordering
/// comparisons (`<`, `>`, `ORDER BY`) on the stored column are meaningless.
impl ToSql for u64 {
    fn to_sql(&self) -> Value {
        Value::Integer(self.cast_signed())
    }
}

impl FromSql for u64 {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Integer(v) => Ok(v.cast_unsigned()),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "integer",
                actual,
            }),
        }
    }
}

impl ToSql for u32 {
    fn to_sql(&self) -> Value {
        Value::Integer(i64::from(*self))
    }
}

impl FromSql for u32 {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Integer(v) => u32::try_from(v).map_err(|_| StorageError::IntegerCast {
                column: idx,
                value: v,
                target: "u32",
            }),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "integer",
                actual,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

impl ToSql for Uuid {
    fn to_sql(&self) -> Value {
        Value::Blob(self.as_bytes().to_vec())
    }
}

impl FromSql for Uuid {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Blob(bytes) => {
                Uuid::from_slice(&bytes).map_err(|_| StorageError::ColumnTypeMismatch {
                    column: idx,
                    expected: "16-byte uuid blob",
                    actual: Value::Blob(bytes),
                })
            }
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "uuid blob",
                actual,
            }),
        }
    }
}

impl ToSql for Timestamp {
    fn to_sql(&self) -> Value {
        Value::Integer(self.as_millisecond())
    }
}

impl FromSql for Timestamp {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Integer(millis) => Timestamp::from_millisecond(millis)
                .map_err(|_| StorageError::InvalidTimestamp { millis }),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "integer",
                actual,
            }),
        }
    }
}

impl ToSql for DbId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for DbId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for Level {
    fn to_sql(&self) -> Value {
        Value::Integer(self.as_db())
    }
}

impl FromSql for Level {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        let v = i64::from_sql(value, idx)?;
        Level::from_db(v)
    }
}

impl ToSql for TaskStatus {
    fn to_sql(&self) -> Value {
        Value::Text(self.as_db().to_owned())
    }
}

impl FromSql for TaskStatus {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Text(s) => Self::from_db(&s),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "text",
                actual,
            }),
        }
    }
}

impl ToSql for Map<String, serde_json::Value> {
    fn to_sql(&self) -> Value {
        Value::Text(serde_json::to_string(self).expect("serializing a JSON map cannot fail"))
    }
}

impl FromSql for Map<String, serde_json::Value> {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        let text = String::from_sql(value, idx)?;
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|err| StorageError::InvalidJson {
                column: idx,
                error: err.to_string(),
            })
            .map(|v| v.as_object().cloned().unwrap_or_default())
    }
}
