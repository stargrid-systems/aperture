//! OS integration for the Aperture gateway.
//!
//! Provides mDNS service publication via Avahi and hostname management via
//! systemd-hostnamed. This crate is pulled in only when the `os-integration`
//! Cargo feature is enabled on the `aperture` binary crate.

pub use self::avahi::{ServicePublisher, ServiceSpec};
pub use self::error::OsError;
pub use self::hostname::{apply_hostname, read_hostname};
pub use self::setting::{SystemDef, SystemValue};

pub use zbus::Connection;

mod avahi;
mod error;
mod hostname;
mod setting;
