//! Settings store: one JSON blob per setting key.
//!
//! Each row holds the serialized value for one setting key. The shape of
//! that value is opaque here. The settings layer owns the typed definition,
//! validation, and (de)serialization, so storage stays a plain key-value
//! record.

use jiff::Timestamp;
use serde_json::Value;
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::sql;
use crate::sql::{Columns, ToSql};

mod col {
    pub const KEY: &str = "key";
    pub const UPDATED_AT: &str = "updated_at";
    pub const UPDATED_BY: &str = "updated_by";
    pub const VALUE: &str = "value";
}

const SETTING_COLUMNS: Columns =
    Columns::new(&[col::KEY, col::VALUE, col::UPDATED_AT, col::UPDATED_BY]);

/// One stored setting value, including its key and audit metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRecord {
    /// The setting key.
    pub key: String,
    /// The serialized value.
    pub value: Value,
    /// When the value was last written.
    pub updated_at: Timestamp,
    /// The actor that last wrote the value.
    pub updated_by: ActorId,
}

/// Repository over the `settings` table.
pub struct SettingRepository {
    connection: Connection,
}

impl SettingRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Returns the stored value for `key`, if one has been written.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, key: &str) -> Result<Option<SettingRecord>> {
        let sql = format!(
            sql!(SELECT {cols} FROM settings WHERE key = ?1),
            cols = SETTING_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([key.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(SettingRecord::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Stores `value` for `key`, replacing any existing value (upsert).
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the write fails.
    #[tracing::instrument(level = "info", skip(self, value))]
    pub async fn put(
        &self,
        key: &str,
        value: &Value,
        updated_by: ActorId,
        updated_at: Timestamp,
    ) -> Result<()> {
        self.connection
            .execute(
                sql!(
                    INSERT INTO settings (key, value, updated_at, updated_by)
                    VALUES (?1, ?2, ?3, ?4)
                    ON CONFLICT (key) DO UPDATE SET
                        value = excluded.value,
                        updated_at = excluded.updated_at,
                        updated_by = excluded.updated_by
                ),
                params_from_iter([
                    key.to_sql(),
                    value.to_sql(),
                    updated_at.to_sql(),
                    updated_by.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Returns every stored setting, ordered by key.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list(&self) -> Result<Vec<SettingRecord>> {
        let sql = format!(
            sql!(SELECT {cols} FROM settings ORDER BY key),
            cols = SETTING_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql, ())
            .await
            .map_err(StorageError::from_turso)?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            records.push(SettingRecord::try_from(&row)?);
        }
        Ok(records)
    }
}

impl TryFrom<&Row> for SettingRecord {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
            key: SETTING_COLUMNS.extract(row, col::KEY)?,
            value: SETTING_COLUMNS.extract(row, col::VALUE)?,
            updated_at: SETTING_COLUMNS.extract(row, col::UPDATED_AT)?,
            updated_by: SETTING_COLUMNS.extract(row, col::UPDATED_BY)?,
        })
    }
}
