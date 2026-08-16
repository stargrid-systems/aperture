//! The event definition trait: a typed, registered event kind.

use serde::Serialize;
use serde::de::DeserializeOwned;
use utoipa::ToSchema;

/// A kind of domain event. The implementing type IS the payload.
///
/// Payloads are (de)serialized at the boundary. Validity is enforced by
/// construction: if a JSON value deserializes into the type, it is accepted.
pub trait EventDefinition:
    DeserializeOwned + Serialize + ToSchema + Default + Send + Sync + 'static
{
    /// The unique key string this definition is registered under, e.g.
    /// `"artifact.written"`.
    const KEY: &'static str;
}
