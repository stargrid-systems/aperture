//! Settings system for the Aperture gateway.
//!
//! A setting is a scoped piece of runtime configuration. Each scope is a
//! [`SettingDefinition`] with a typed value, registered once in a
//! [`SettingRegistry`]. [`Settings`] reads and writes values, filling from the
//! definition default when no value has been stored.

pub use aperture_storage::{ActorId, SettingRecord, SettingRepository};

pub use self::definition::SettingDefinition;
pub use self::erased::ErasedSettingDefinition;
pub use self::error::SettingError;
pub use self::registry::{SettingDescriptor, SettingRegistry};
pub use self::settings::Settings;

mod definition;
mod erased;
mod error;
mod registry;
mod settings;
