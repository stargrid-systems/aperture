//! Settings system for the Aperture gateway.
//!
//! A setting is a scoped piece of runtime configuration. Each scope is a
//! [`SettingDefinition`] with a typed value, registered once in a
//! [`SettingRegistry`]. [`Settings`] reads and writes values, filling from the
//! definition default when no value has been stored.

use aperture_runtime::Registry;
pub use aperture_storage::{ActorId, SettingRecord, SettingRepository};

pub use self::change::{SettingChange, SettingChangeError};
pub use self::definition::SettingDefinition;
pub use self::erased::ErasedSettingDefinition;
pub use self::error::SettingError;
pub use self::settings::Settings;

mod change;
mod definition;
mod erased;
mod error;
mod settings;

/// The registry of setting definitions, keyed by setting key.
pub type SettingRegistry = Registry<dyn ErasedSettingDefinition>;
