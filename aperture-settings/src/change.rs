//! Change feed for the settings store.
//!
//! [`Settings::subscribe`] returns a [`Receiver`] that observes every
//! successful write. The feed is in-process and best-effort: late subscribers
//! do not see events from before they subscribed, and a full channel drops
//! events (the next subscriber-visible event still arrives).
//!
//! Use the feed to react to setting changes without coupling writers to
//! consumers.
//!
//! [`Settings::subscribe`]: crate::Settings::subscribe
//! [`Receiver`]: tokio::sync::broadcast::Receiver

use serde_json::Value;

/// A setting was written.
#[derive(Debug, Clone, PartialEq)]
#[expect(clippy::derive_partial_eq_without_eq)]
pub struct SettingChange {
    /// The key that changed.
    pub key: String,
    /// The new value.
    pub value: Value,
}
