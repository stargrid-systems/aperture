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

use crate::definition::SettingDefinition;

/// A setting was written.
#[derive(Debug, Clone, PartialEq)]
#[expect(clippy::derive_partial_eq_without_eq)]
pub struct SettingChange {
    /// The key that changed.
    pub key: String,
    /// The new value.
    pub value: Value,
}

impl SettingChange {
    /// Decodes the change as setting `D`. Returns `Ok(None)` if the key does
    /// not match `D::KEY`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the key matches but the value fails to deserialize.
    pub fn decode_as<D: SettingDefinition>(&self) -> Result<Option<D::Value>, serde_json::Error> {
        if self.key == D::KEY {
            serde_json::from_value(self.value.clone()).map(Some)
        } else {
            Ok(None)
        }
    }
}
