//! The setting definition trait: a typed, registered scope of configuration.

use serde::de::DeserializeOwned;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::SettingError;

/// A kind of setting. Each definition fixes a unique [`SettingDefinition::KEY`]
/// and a typed `Value`. The value is validated and (de)serialized at the
/// boundary, so callers only ever see typed values.
pub trait SettingDefinition: Send + Sync + 'static {
    /// The unique scope string this definition is registered under.
    const KEY: &'static str;
    /// The typed value the scope holds.
    type Value: DeserializeOwned + Serialize + ToSchema + Send;

    /// The default value used when no value has been stored yet.
    fn default(&self) -> Self::Value;

    /// Validates `value` before it is written. Returns
    /// [`SettingError::Invalid`] on rejection. The default implementation
    /// accepts everything.
    ///
    /// # Errors
    ///
    /// Returns `SettingError::Invalid` if the value is not acceptable.
    fn validate(&self, value: &Self::Value) -> Result<(), SettingError> {
        let _ = value;
        Ok(())
    }
}
