//! The registry of setting definitions.
//!
//! Definitions are registered once at startup. The registry is the single
//! source of truth for what keys exist: [`SettingRegistry`] looks up
//! definitions by key, and the HTTP layer projects
//! [`SettingRegistry::descriptors`] into the `OpenAPI` document.

use std::sync::Arc;

use aperture_storage::Registry;
use utoipa::openapi::schema::Schema;
use utoipa::openapi::RefOr;

use crate::definition::SettingDefinition;
use crate::erased::ErasedSettingDefinition;

/// A public, schema-carrying description of one registered key.
pub struct SettingDescriptor {
    /// The key string.
    pub key: &'static str,
    /// Component name of the key's value type.
    pub value_name: String,
    /// Schema of the key's value type.
    pub value_schema: RefOr<Schema>,
    /// Named component schemas the value type references, including itself and
    /// its dependencies.
    pub schemas: Vec<(String, RefOr<Schema>)>,
}

/// A registry of setting definitions, keyed by setting key.
#[derive(Default)]
pub struct SettingRegistry(Registry<dyn ErasedSettingDefinition>);

impl SettingRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `definition` under its [`SettingDefinition::KEY`], replacing
    /// any previously registered definition for that key.
    pub fn register<T: SettingDefinition>(&mut self, definition: T) {
        self.0.register(T::KEY, Arc::new(definition));
    }

    /// A schema-carrying descriptor per registered key.
    pub fn descriptors(&self) -> impl Iterator<Item = SettingDescriptor> + '_ {
        self.0.values().map(|definition| {
            let mut schemas = Vec::new();
            definition.collect_schemas(&mut schemas);
            SettingDescriptor {
                key: definition.key(),
                value_name: definition.value_name(),
                value_schema: definition.value_schema(),
                schemas,
            }
        })
    }

    /// Iterates over the registered keys.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.keys()
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Arc<dyn ErasedSettingDefinition>> {
        self.0.get(key)
    }
}
