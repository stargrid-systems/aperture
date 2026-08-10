//! [`Settings`]: reads and writes scoped configuration values.
//!
//! Each scope is a [`SettingDefinition`] registered in a
//! [`SettingRegistry`]. Values are persisted as JSON in the storage catalog.
//! Reads fill from the definition default when no value has been stored, so a
//! fresh install always returns a complete configuration.

use std::sync::Arc;

use aperture_storage::{ActorId, SettingRepository};
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
    /// Creates a settings service backed by `repo` and the scopes in
    /// `registry`.
    pub fn new(repo: SettingRepository, registry: SettingRegistry) -> Self {
        Self {
            inner: Arc::new(SettingsInner { repo, registry }),
        }
    }

    /// The registry of scopes, for projecting schemas.
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
            Some(record) => Ok(record.data),
            None => Ok(definition.default_value()),
        }
    }

    /// Returns the typed value for the scope `D`, filling from the definition
    /// default when no value has been stored.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::NotRegistered` if the scope is unknown,
    /// `SettingError::Decode` if the stored value cannot be decoded, or a
    /// storage error if the read fails.
    pub async fn get<D>(&self) -> Result<D::Value, SettingError>
    where
        D: SettingDefinition,
    {
        let value = self.get_value(D::KEY).await?;
        serde_json::from_value(value).map_err(SettingError::Decode)
    }

    /// Validates and stores `value` for `key`, recording `updated_by` as the
    /// actor that wrote it.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::NotRegistered` if `key` is unknown,
    /// `SettingError::Invalid` if validation rejects the value, or a storage
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
        definition.validate(&value)?;
        let now = Timestamp::now();
        self.inner.repo.put(key, &value, updated_by, now).await?;
        Ok(())
    }

    /// Returns every registered scope with its current value (or default).
    /// Ordered by scope key.
    ///
    /// # Errors
    ///
    /// Returns a storage error if any read fails.
    pub async fn list(&self) -> Result<Vec<(String, Value)>, SettingError> {
        let mut scopes: Vec<&'static str> = self.inner.registry.keys();
        scopes.sort_unstable();
        let mut result = Vec::with_capacity(scopes.len());
        for key in scopes {
            let value = self.get_value(key).await?;
            result.push((key.to_owned(), value));
        }
        Ok(result)
    }
}
