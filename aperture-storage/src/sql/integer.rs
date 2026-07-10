use turso::Value;

use super::{FromSql, ToSql};
use crate::{Result, StorageError};

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
