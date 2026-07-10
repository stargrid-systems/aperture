use turso::Value;

use super::{FromSql, ToSql};
use crate::{Result, StorageError};

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
