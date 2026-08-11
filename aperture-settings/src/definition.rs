//! The setting definition trait: a typed, registered piece of configuration.

use serde::Serialize;
use serde::de::DeserializeOwned;
use utoipa::ToSchema;

/// A kind of setting. Each definition fixes a unique [`SettingDefinition::KEY`]
/// and a typed `Value`.
///
/// The value is (de)serialized at the boundary. Validity is enforced by
/// construction: if a JSON value deserializes into the `Value` type, it is
/// accepted.
///
/// For constraints beyond what the type system expresses (e.g. a non-empty
/// hostname), use a newtype or custom `Deserialize` impl so the type itself
/// rejects invalid values.
pub trait SettingDefinition: Send + Sync + 'static {
    /// The unique key string this definition is registered under.
    const KEY: &'static str;
    /// The typed value the setting holds.
    type Value: DeserializeOwned + Serialize + ToSchema + Send;

    /// The default value used when no value has been stored yet.
    fn default(&self) -> Self::Value;
}
