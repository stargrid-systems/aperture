//! [`Settings`]: reads and writes keyed configuration values.
//!
//! Each key is a [`SettingDefinition`] registered in a
//! [`SettingRegistry`]. Values are persisted as JSON in the storage catalog.
//! Reads fill from the definition default when no value has been stored, so a
//! fresh install always returns a complete configuration.

use std::sync::Arc;

use aperture_storage::{ActorId, SettingRepository};
use jiff::Timestamp;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::SettingRegistry;
use crate::change::SettingChange;
use crate::definition::SettingDefinition;
use crate::error::SettingError;

/// Capacity of the in-process change feed.
const CHANGE_FEED_CAPACITY: usize = 64;

struct SettingsInner {
    repo: SettingRepository,
    registry: SettingRegistry,
    changes: broadcast::Sender<SettingChange>,
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
        let (changes, _) = broadcast::channel(CHANGE_FEED_CAPACITY);
        Self {
            inner: Arc::new(SettingsInner {
                repo,
                registry,
                changes,
            }),
        }
    }

    /// Subscribes to setting changes.
    pub fn subscribe(&self) -> broadcast::Receiver<SettingChange> {
        self.inner.changes.subscribe()
    }

    /// The registry of definitions, for listings and schema lookup.
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
    pub async fn get<D>(&self) -> Result<D, SettingError>
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
        let _ = self.inner.changes.send(SettingChange {
            key: key.to_owned(),
            value,
            actor: updated_by,
            timestamp: now,
        });
        Ok(())
    }

    /// Returns every registered key with its current value (or default).
    /// Ordered by key.
    ///
    /// # Errors
    ///
    /// Returns a storage error if any read fails.
    pub async fn list(&self) -> Result<Vec<(String, Value)>, SettingError> {
        let registry = &self.inner.registry;
        let mut result = Vec::with_capacity(registry.len());
        for key in registry.keys() {
            let value = self.get_value(key).await?;
            result.push((key.to_owned(), value));
        }
        Ok(result)
    }
}
