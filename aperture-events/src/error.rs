//! Errors returned by event operations.

#[derive(Debug, thiserror::Error)]
pub enum EventError {
    /// The recorder channel is closed: no [`crate::EventRecorder`] is
    /// draining the bus, or it has already shut down.
    #[error("event recorder is not running")]
    RecorderClosed,
}
