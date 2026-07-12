use jiff::Timestamp;
use turso::Value;
use uuid::Uuid;

use super::{FromSql, ToSql};
use crate::{
    ActorId, ActorKind, ApiKeyId, ArtifactId, DbId, EventId, Level, Result, SessionId, SpanId,
    StorageError, TaskId, TaskStatus, UserId,
};

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

impl ToSql for ActorId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for ActorId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for UserId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for UserId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for SessionId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for SessionId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for ApiKeyId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for ApiKeyId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for TaskId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for TaskId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for ArtifactId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for ArtifactId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for SpanId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for SpanId {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        i64::from_sql(value, idx).map(Self::from)
    }
}

impl ToSql for EventId {
    fn to_sql(&self) -> Value {
        Value::Integer(self.get())
    }
}

impl FromSql for EventId {
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
