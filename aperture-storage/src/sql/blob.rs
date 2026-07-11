use turso::Value;

use super::{FromSql, ToSql};
use crate::{Result, StorageError};

impl ToSql for [u8] {
    fn to_sql(&self) -> Value {
        Value::Blob(self.to_vec())
    }
}

impl ToSql for Vec<u8> {
    fn to_sql(&self) -> Value {
        Value::Blob(self.clone())
    }
}

impl FromSql for Vec<u8> {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Blob(bytes) => Ok(bytes),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "blob",
                actual,
            }),
        }
    }
}
