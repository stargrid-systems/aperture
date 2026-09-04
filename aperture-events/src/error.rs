//! Errors returned by event operations.

#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error(transparent)]
    Storage(#[from] aperture_storage::StorageError),
    #[error("failed to serialize event payload")]
    Serialize(#[source] serde_json::Error),
}
