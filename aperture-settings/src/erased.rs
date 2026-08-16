//! Type-erased view of a [`SettingDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many keys
//! together. Erasure happens only here: JSON is decoded into the key's typed
//! value on the way in, and the typed value is encoded back out. A blanket
//! impl bridges every [`SettingDefinition`].

use aperture_runtime::{RegistryEntry, json_schema};
use serde_json::Value;

use crate::definition::SettingDefinition;
use crate::error::SettingError;

pub trait ErasedSettingDefinition: Send + Sync + 'static {
    /// The key this definition is registered under.
    fn key(&self) -> &'static str;
    /// A standalone JSON Schema document of the key's value type.
    fn value_schema(&self) -> Value;
    /// Returns the default value as JSON.
    fn default_value(&self) -> Value;
    /// Checks that `value` deserializes into this key's value type.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::Decode` if the value does not fit the type.
    fn check_value(&self, value: &Value) -> Result<(), SettingError>;
}

impl RegistryEntry for dyn ErasedSettingDefinition {
    fn key(&self) -> &'static str {
        ErasedSettingDefinition::key(self)
    }
}

impl<T: SettingDefinition> ErasedSettingDefinition for T {
    fn key(&self) -> &'static str {
        T::KEY
    }

    fn value_schema(&self) -> Value {
        json_schema::<T>()
    }

    fn default_value(&self) -> Value {
        serde_json::to_value(T::default()).expect("default value must serialize")
    }

    fn check_value(&self, value: &Value) -> Result<(), SettingError> {
        serde_json::from_value::<T>(value.clone()).map_err(SettingError::Decode)?;
        Ok(())
    }
}
