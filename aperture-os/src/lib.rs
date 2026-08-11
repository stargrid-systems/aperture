//! OS integration for the Aperture gateway.
//!
//! Provides mDNS service publication via Avahi, hostname management via
//! systemd-hostnamed, and hostname task definitions. This crate is pulled in
//! only when the `os-integration` Cargo feature is enabled on the `aperture`
//! binary crate.

pub use self::avahi::{ServicePublisher, ServiceSpec};
pub use self::error::{HostnameError, OsError};
pub use self::hostname::{
    ApplyHostnameDefinition, ApplyHostnameInput, ApplyHostnameOutput, ReadHostnameDefinition,
    ReadHostnameInput, ReadHostnameOutput, apply_hostname, read_hostname,
};
pub use self::setting::{Hostname, HostnameDef};
pub use zbus::Connection;

mod avahi;
mod error;
mod hostname;
mod setting;
