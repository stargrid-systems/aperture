//! The setting definition trait: a typed, registered piece of configuration.

use serde::Serialize;
use serde::de::DeserializeOwned;
use utoipa::ToSchema;

/// A kind of setting. The implementing type IS the value: it carries its
/// unique [`KEY`](Self::KEY) and is (de)serialized at the boundary.
///
/// Validity is enforced by construction: if a JSON value deserializes into
/// the type, it is accepted. For constraints beyond what the type system
/// expresses (e.g. a non-empty hostname), use a newtype or custom `Deserialize`
/// impl so the type itself rejects invalid values.
pub trait SettingDefinition:
    DeserializeOwned + Serialize + ToSchema + Default + Send + Sync + 'static
{
    /// The unique key string this definition is registered under.
    const KEY: &'static str;
}
