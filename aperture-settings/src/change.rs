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

use aperture_storage::ActorId;
use serde_json::Value;

use crate::definition::SettingDefinition;

/// A setting was written.
#[derive(Debug, Clone, PartialEq)]
#[expect(clippy::derive_partial_eq_without_eq)]
pub struct SettingChange {
    pub(crate) key: String,
    pub(crate) value: Value,
    pub(crate) actor: ActorId,
}

impl SettingChange {
    /// Checks whether this change is for setting `D`.
    pub fn is<D: SettingDefinition>(&self) -> bool {
        self.key == D::KEY
    }

    /// The actor that performed the change.
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// Decodes the change as setting `D`.
    ///
    /// Returns `None` if the key does not match `D::KEY`. If the key matches,
    /// the value is expected to decode successfully; a decode failure panics
    /// because the internal representation should always be valid.
    ///
    /// # Panics
    ///
    /// Panics if the key matches but the value fails to deserialize.
    pub fn decode<D: SettingDefinition>(&self) -> Option<D> {
        if self.key != D::KEY {
            return None;
        }
        Some(
            serde_json::from_value(self.value.clone())
                .expect("setting value must decode when key matches"),
        )
    }

    /// Attempts to decode the change as setting `D`.
    ///
    /// Fails if the key does not match or if the value fails to deserialize.
    ///
    /// # Errors
    ///
    /// Returns [`SettingChangeError::KeyMismatch`] if the key does not match,
    /// or [`SettingChangeError::Decode`] if the value fails to deserialize.
    pub fn try_decode<D: SettingDefinition>(&self) -> Result<D, SettingChangeError> {
        if self.key != D::KEY {
            return Err(SettingChangeError::KeyMismatch);
        }
        Ok(serde_json::from_value(self.value.clone())?)
    }
}

/// Errors from decoding a [`SettingChange`].
#[derive(Debug, thiserror::Error)]
pub enum SettingChangeError {
    /// The change key does not match the requested setting.
    #[error("key mismatch")]
    KeyMismatch,
    /// The value failed to deserialize.
    #[error("failed to decode setting value")]
    Decode(#[source] serde_json::Error),
}

impl From<serde_json::Error> for SettingChangeError {
    fn from(err: serde_json::Error) -> Self {
        Self::Decode(err)
    }
}
