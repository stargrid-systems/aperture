//! Shared helpers for binding values and decoding rows.
//!
//! Repositories convert between domain types and turso [`Value`]s through
//! these, so the `NULL` handling and the decode error messages stay consistent.

use jiff::Timestamp;
use turso::{Row, Value};

use crate::error::{Result, StorageError, database};

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

pub(crate) fn int_or_null<T: Into<i64>>(value: Option<T>) -> Value {
    match value {
        Some(int) => Value::Integer(int.into()),
        None => Value::Null,
    }
}

pub(crate) fn ts_or_null(value: Option<Timestamp>) -> Value {
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
