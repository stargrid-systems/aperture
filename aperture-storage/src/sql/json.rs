use turso::Value;

use super::{FromSql, ToSql};
use crate::{Result, StorageError};

impl ToSql for serde_json::Value {
    fn to_sql(&self) -> Value {
        Value::Text(serde_json::to_string(self).expect("serializing a JSON value cannot fail"))
    }
}

impl FromSql for serde_json::Value {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        let text = String::from_sql(value, idx)?;
        serde_json::from_str(&text).map_err(|err| StorageError::InvalidJson {
            column: idx,
            error: err.to_string(),
        })
    }
}

impl ToSql for serde_json::Map<String, serde_json::Value> {
    fn to_sql(&self) -> Value {
        Value::Text(serde_json::to_string(self).expect("serializing a JSON map cannot fail"))
    }
}

impl FromSql for serde_json::Map<String, serde_json::Value> {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        let text = String::from_sql(value, idx)?;
        let value = serde_json::from_str::<serde_json::Value>(&text).map_err(|err| {
            StorageError::InvalidJson {
                column: idx,
                error: err.to_string(),
            }
        })?;
        match value {
            serde_json::Value::Object(map) => Ok(map),
            other => Err(StorageError::InvalidJson {
                column: idx,
                error: format!("expected a JSON object, got {}", other),
            }),
        }
    }
}
