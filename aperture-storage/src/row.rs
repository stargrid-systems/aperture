//! Shared helpers for binding values and decoding rows.
//!
//! Repositories convert between domain types and turso [`Value`]s through
//! these, so the `NULL` handling and the decode error messages stay consistent.

use jiff::Timestamp;
use serde_json::Map;
use turso::{Row, Value};
use uuid::Uuid;

use crate::error::{Result, StorageError, database};
use crate::id::DbId;

pub(crate) fn text_or_null(value: &Option<String>) -> Value {
    match value {
        Some(text) => Value::Text(text.clone()),
        None => Value::Null,
    }
}

pub(crate) fn text_ref_or_null(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::Text(text.to_owned()),
        None => Value::Null,
    }
}

pub(crate) fn uuid_or_null(value: Option<Uuid>) -> Value {
    match value {
        Some(uuid) => Value::Blob(uuid.as_bytes().to_vec()),
        None => Value::Null,
    }
}

pub(crate) fn json_text_or_null(
    value: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Value {
    match value {
        Some(map) => {
            Value::Text(serde_json::to_string(map).expect("serializing a JSON map cannot fail"))
        }
        None => Value::Null,
    }
}

pub(crate) fn int_or_null<T: Into<i64>>(value: Option<T>) -> Value {
    match value {
        Some(int) => Value::Integer(int.into()),
        None => Value::Null,
    }
}

/// Bitwise cast from u64 to i64 for database storage.
///
/// SQLite stores integers as signed 64-bit. Values above `i64::MAX` wrap to
/// negative numbers. Equality comparisons are preserved, but ordering
/// comparisons (`<`, `>`, `ORDER BY`) on the stored column are meaningless.
pub(crate) fn int_u64(value: u64) -> i64 {
    value.cast_signed()
}

/// See the note on [`int_u64`].
pub(crate) fn int_or_null_u64(value: Option<u64>) -> Value {
    match value {
        Some(v) => Value::Integer(int_u64(v)),
        None => Value::Null,
    }
}

pub(crate) fn int_or_null_ts(value: Option<Timestamp>) -> Value {
    match value {
        Some(timestamp) => Value::Integer(timestamp.as_millisecond()),
        None => Value::Null,
    }
}

pub(crate) fn req_text(row: &Row, idx: usize) -> Result<String> {
    match row.get_value(idx).map_err(database)? {
        Value::Text(text) => Ok(text),
        actual => Err(StorageError::ColumnTypeMismatch {
            column: idx,
            expected: "text",
            actual,
        }),
    }
}

pub(crate) fn opt_text(row: &Row, idx: usize) -> Result<Option<String>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text)),
        actual => Err(StorageError::ColumnTypeMismatch {
            column: idx,
            expected: "text or null",
            actual,
        }),
    }
}

pub(crate) fn opt_uuid(row: &Row, idx: usize) -> Result<Option<Uuid>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Blob(bytes) => Ok(Some(Uuid::from_slice(&bytes).map_err(|_| {
            StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "16-byte uuid blob",
                actual: Value::Blob(bytes),
            }
        })?)),
        actual => Err(StorageError::ColumnTypeMismatch {
            column: idx,
            expected: "uuid blob or null",
            actual,
        }),
    }
}

pub(crate) fn json_map(row: &Row, idx: usize) -> Result<Map<String, serde_json::Value>> {
    let Some(text) = opt_text(row, idx)? else {
        return Ok(Map::new());
    };
    Ok(serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|err| StorageError::InvalidJson {
            column: idx,
            error: err.to_string(),
        })?
        .as_object()
        .cloned()
        .unwrap_or_default())
}

pub(crate) fn req_db_id(row: &Row, idx: usize) -> Result<DbId> {
    Ok(DbId::from(req_int(row, idx)?))
}

pub(crate) fn opt_db_id(row: &Row, idx: usize) -> Result<Option<DbId>> {
    Ok(opt_int(row, idx)?.map(DbId::from))
}

pub(crate) fn req_int(row: &Row, idx: usize) -> Result<i64> {
    match row.get_value(idx).map_err(database)? {
        Value::Integer(int) => Ok(int),
        actual => Err(StorageError::ColumnTypeMismatch {
            column: idx,
            expected: "integer",
            actual,
        }),
    }
}

pub(crate) fn req_u64(row: &Row, idx: usize) -> Result<u64> {
    req_int(row, idx).and_then(|v| {
        u64::try_from(v).map_err(|_| StorageError::IntegerCast {
            column: idx,
            value: v,
            target: "u64",
        })
    })
}

pub(crate) fn opt_int(row: &Row, idx: usize) -> Result<Option<i64>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Integer(int) => Ok(Some(int)),
        actual => Err(StorageError::ColumnTypeMismatch {
            column: idx,
            expected: "integer or null",
            actual,
        }),
    }
}

pub(crate) fn opt_u32(row: &Row, idx: usize) -> Result<Option<u32>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Integer(int) => {
            u32::try_from(int)
                .map(Some)
                .map_err(|_| StorageError::IntegerCast {
                    column: idx,
                    value: int,
                    target: "u32",
                })
        }
        actual => Err(StorageError::ColumnTypeMismatch {
            column: idx,
            expected: "integer or null",
            actual,
        }),
    }
}

pub(crate) fn req_ts(row: &Row, idx: usize) -> Result<Timestamp> {
    ts_from_millis(req_int(row, idx)?)
}

pub(crate) fn opt_ts(row: &Row, idx: usize) -> Result<Option<Timestamp>> {
    match opt_int(row, idx)? {
        Some(millis) => Ok(Some(ts_from_millis(millis)?)),
        None => Ok(None),
    }
}

fn ts_from_millis(millis: i64) -> Result<Timestamp> {
    Timestamp::from_millisecond(millis).map_err(|_| StorageError::InvalidTimestamp { millis })
}
