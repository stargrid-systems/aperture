use turso::Value;

use super::{FromSql, ToSql};
use crate::{Result, StorageError};

impl ToSql for bool {
    fn to_sql(&self) -> Value {
        Value::Integer(if *self { 1 } else { 0 })
    }
}

impl FromSql for bool {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Integer(0) => Ok(false),
            Value::Integer(1) => Ok(true),
            Value::Integer(v) => Err(StorageError::IntegerCast {
                column: idx,
                value: v,
                target: "bool",
            }),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "integer (0 or 1)",
                actual,
            }),
        }
    }
}
