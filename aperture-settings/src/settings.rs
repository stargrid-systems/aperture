//! [`Settings`]: reads and writes keyed configuration values.
//!
//! Each key is a [`SettingDefinition`] registered in a
//! [`SettingRegistry`]. Values are persisted as JSON in the storage catalog.
//! Reads fill from the definition default when no value has been stored, so a
//! fresh install always returns a complete configuration.

use std::sync::Arc;

use aperture_storage::{
    ActorId, CursorValue, ListQuery, Order, Page, Paginator, SettingRepository,
};
use jiff::Timestamp;
use serde_json::Value;

use crate::definition::SettingDefinition;
use crate::error::SettingError;
use crate::registry::SettingRegistry;

struct SettingsInner {
    repo: SettingRepository,
    registry: SettingRegistry,
}

/// Reads and writes setting values. Cheap to clone: all clones share one
/// instance.
#[derive(Clone)]
pub struct Settings {
    inner: Arc<SettingsInner>,
}

impl Settings {
    /// Creates a settings service backed by `repo` and the keys in
    /// `registry`.
    pub fn new(repo: SettingRepository, registry: SettingRegistry) -> Self {
        Self {
            inner: Arc::new(SettingsInner { repo, registry }),
        }
    }

    /// The registry of keys, for projecting schemas.
    pub fn registry(&self) -> &SettingRegistry {
        &self.inner.registry
    }

    /// Returns the value for `key` as typed JSON, filling from the definition
    /// default when no value has been stored.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::NotRegistered` if `key` is unknown, or a storage
    /// error if the read fails.
    pub async fn get_value(&self, key: &str) -> Result<Value, SettingError> {
        let definition = self
            .inner
            .registry
            .get(key)
            .ok_or_else(|| SettingError::NotRegistered(key.to_owned()))?;
        match self.inner.repo.get(key).await? {
            Some(record) => Ok(record.value),
            None => Ok(definition.default_value()),
        }
    }

    /// Returns the typed value for the key `D`, filling from the definition
    /// default when no value has been stored.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::NotRegistered` if the key is unknown,
    /// `SettingError::Decode` if the stored value cannot be decoded, or a
    /// storage error if the read fails.
    pub async fn get<D>(&self) -> Result<D::Value, SettingError>
    where
        D: SettingDefinition,
    {
        let value = self.get_value(D::KEY).await?;
        serde_json::from_value(value).map_err(SettingError::Decode)
    }

    /// Stores `value` for `key`, recording `updated_by` as the actor that
    /// wrote it. The value must deserialize into the key's value type.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::NotRegistered` if `key` is unknown,
    /// `SettingError::Decode` if the value does not fit the type, or a storage
    /// error if the write fails.
    pub async fn set_value(
        &self,
        key: &str,
        value: Value,
        updated_by: ActorId,
    ) -> Result<(), SettingError> {
        let definition = self
            .inner
            .registry
            .get(key)
            .ok_or_else(|| SettingError::NotRegistered(key.to_owned()))?;
        definition.check_value(&value)?;
        let now = Timestamp::now();
        self.inner.repo.put(key, &value, updated_by, now).await?;
        Ok(())
    }

    /// Lists registered keys with their current values (or defaults),
    /// paginated by key.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::Storage` if the cursor is invalid or a read
    /// fails.
    pub async fn list(&self, query: &ListQuery) -> Result<Page<(String, Value)>, SettingError> {
        let paginator = Paginator::new(query, Order::Asc)?;
        let order = paginator.query_order();

        let mut keys: Vec<String> = self.inner.registry.keys().map(String::from).collect();
        match order {
            Order::Asc => keys.sort_unstable(),
            Order::Desc => keys.sort_unstable_by(|a, b| b.cmp(a)),
        }

        if let Some(cursor) = paginator.cursor()
            && let CursorValue::Text(cursor_key) = cursor.value()
        {
            match order {
                Order::Asc => keys.retain(|k| k > cursor_key),
                Order::Desc => keys.retain(|k| k < cursor_key),
            }
        }

        let limit = paginator.fetch_limit() as usize;
        let page_keys: Vec<String> = keys.into_iter().take(limit).collect();

        let mut entries = Vec::with_capacity(page_keys.len());
        for key in &page_keys {
            let value = self.get_value(key).await?;
            entries.push((key.clone(), value));
        }

        Ok(paginator.finish(entries, |(key, _)| (CursorValue::Text(key.clone()), 0)))
    }
}
