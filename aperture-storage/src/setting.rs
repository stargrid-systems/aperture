//! Settings store: one JSON blob per scope.
//!
//! Each row holds the serialized value for one setting scope. The shape of
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
    pub const DATA: &str = "data";
    pub const SCOPE: &str = "scope";
    pub const UPDATED_AT: &str = "updated_at";
    pub const UPDATED_BY: &str = "updated_by";
}

const SETTING_COLUMNS: Columns = Columns::new(&[
    col::SCOPE,
    col::DATA,
    col::UPDATED_AT,
    col::UPDATED_BY,
]);

/// One stored setting value, including its scope key and audit metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRecord {
    /// The scope this value belongs to.
    pub scope: String,
    /// The serialized value.
    pub data: Value,
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

    /// Returns the stored value for `scope`, if one has been written.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, scope: &str) -> Result<Option<SettingRecord>> {
        let sql = format!(
            sql!(SELECT {cols} FROM settings WHERE scope = ?1),
            cols = SETTING_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([scope.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(SettingRecord::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Stores `data` for `scope`, replacing any existing value (upsert).
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the write fails.
    #[tracing::instrument(level = "info", skip(self, data))]
    pub async fn put(
        &self,
        scope: &str,
        data: &Value,
        updated_by: ActorId,
        updated_at: Timestamp,
    ) -> Result<()> {
        self.connection
            .execute(
                sql!(
                    INSERT INTO settings (scope, data, updated_at, updated_by)
                    VALUES (?1, ?2, ?3, ?4)
                    ON CONFLICT (scope) DO UPDATE SET
                        data = excluded.data,
                        updated_at = excluded.updated_at,
                        updated_by = excluded.updated_by
                ),
                params_from_iter([
                    scope.to_sql(),
                    data.to_sql(),
                    updated_at.to_sql(),
                    updated_by.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Returns every stored setting, ordered by scope.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list(&self) -> Result<Vec<SettingRecord>> {
        let sql = format!(
            sql!(SELECT {cols} FROM settings ORDER BY scope),
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
            scope: SETTING_COLUMNS.extract(row, col::SCOPE)?,
            data: SETTING_COLUMNS.extract(row, col::DATA)?,
            updated_at: SETTING_COLUMNS.extract(row, col::UPDATED_AT)?,
            updated_by: SETTING_COLUMNS.extract(row, col::UPDATED_BY)?,
        })
    }
}
