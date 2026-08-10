//! Type-erased view of a [`SettingDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many scopes
//! together. Erasure happens only here: JSON is decoded into the scope's typed
//! value on the way in, and the typed value is encoded back out. A blanket impl
//! bridges every [`SettingDefinition`].

use serde_json::Value;
use utoipa::openapi::schema::Schema;
use utoipa::openapi::RefOr;
use utoipa::{PartialSchema, ToSchema};

use crate::definition::SettingDefinition;
use crate::error::SettingError;

pub trait ErasedSettingDefinition: Send + Sync + 'static {
    fn key(&self) -> &'static str;
    fn value_name(&self) -> String;
    fn value_schema(&self) -> RefOr<Schema>;
    /// Pushes the named component schemas this scope references (its value
    /// type plus dependencies) into `out`.
    fn collect_schemas(&self, out: &mut Vec<(String, RefOr<Schema>)>);
    /// Returns the default value as JSON.
    fn default_value(&self) -> Value;
    /// Validates that `value` is acceptable for this scope.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::Invalid` if the value fails validation, or
    /// `SettingError::Decode` if it cannot be deserialized.
    fn validate(&self, value: &Value) -> Result<(), SettingError>;
}

impl<T: SettingDefinition> ErasedSettingDefinition for T {
    fn key(&self) -> &'static str {
        T::KEY
    }

    fn value_name(&self) -> String {
        <T::Value as ToSchema>::name().into_owned()
    }

    fn value_schema(&self) -> RefOr<Schema> {
        <T::Value as PartialSchema>::schema()
    }

    fn collect_schemas(&self, out: &mut Vec<(String, RefOr<Schema>)>) {
        out.push((
            <T::Value as ToSchema>::name().into_owned(),
            <T::Value as PartialSchema>::schema(),
        ));
        <T::Value as ToSchema>::schemas(out);
    }

    fn default_value(&self) -> Value {
        serde_json::to_value(SettingDefinition::default(self))
            .expect("default value must serialize")
    }

    fn validate(&self, value: &Value) -> Result<(), SettingError> {
        let typed: T::Value =
            serde_json::from_value(value.clone()).map_err(SettingError::Decode)?;
        SettingDefinition::validate(self, &typed)
    }
}
