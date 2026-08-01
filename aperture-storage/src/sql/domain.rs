use jiff::Timestamp;
use turso::Value;
use uuid::Uuid;

use super::{FromSql, ToSql};
use crate::{ActorKind, Level, Result, StorageError, TaskStatus};

impl ToSql for Uuid {
    fn to_sql(&self) -> Value {
        // Unfortunately we can't yet send the UUID directly as 16 bytes.
        // See: <https://github.com/tursodatabase/turso/issues/6221>.
        Value::Text(self.to_string())
    }
}

impl FromSql for Uuid {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Text(raw) => {
                Self::parse_str(&raw).map_err(|_| StorageError::ColumnTypeMismatch {
                    column: idx,
                    expected: "uuid",
                    actual: Value::Text(raw),
                })
            }
            Value::Blob(bytes) => {
                Self::from_slice(&bytes).map_err(|_| StorageError::ColumnTypeMismatch {
                    column: idx,
                    expected: "16-byte uuid blob",
                    actual: Value::Blob(bytes),
                })
            }
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "uuid",
                actual,
            }),
        }
    }
}

impl ToSql for Timestamp {
    fn to_sql(&self) -> Value {
        Value::Integer(self.as_microsecond())
    }
}

impl FromSql for Timestamp {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Integer(micros) => Self::from_microsecond(micros)
                .map_err(|_| StorageError::InvalidTimestamp { micros }),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "integer",
                actual,
            }),
        }
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
        Self::from_db(v)
    }
}

impl ToSql for ActorKind {
    fn to_sql(&self) -> Value {
        Value::Text(self.as_db().to_owned())
    }
}

impl FromSql for ActorKind {
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
