//! Error types for the settings system.

use aperture_storage::StorageError;

/// Errors from operating the settings system: resolving keys, moving values
/// across the typed boundary, and talking to storage.
#[derive(Debug, thiserror::Error)]
pub enum SettingError {
    /// No definition is registered for the requested key.
    #[error("no setting definition registered for key {0:?}")]
    NotRegistered(String),
    /// The value could not be decoded into the key's value type.
    #[error("failed to decode setting value")]
    Decode(#[source] serde_json::Error),
    /// The value could not be encoded to JSON.
    #[error("failed to encode setting value")]
    Encode(#[source] serde_json::Error),
    /// A storage operation failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
}
