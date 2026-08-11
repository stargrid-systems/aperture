//! OS integration for the Aperture gateway.
//!
//! Provides mDNS service publication via Avahi and hostname management via
//! systemd-hostnamed. This crate is pulled in only when the `os-integration`
//! Cargo feature is enabled on the `aperture` binary crate.

pub use self::avahi::{ServicePublisher, ServiceSpec};
pub use self::error::{HostnameError, OsError};
pub use self::hostname::{ApplyHostnameDefinition, ApplyHostnameInput, ApplyHostnameOutput, apply_hostname};
pub use self::setting::{Hostname, HostnameDef};

mod avahi;
mod error;
mod hostname;
mod setting;

/// A connection to the system D-Bus.
///
/// This is a thin wrapper that hides the underlying transport library.
#[derive(Clone)]
pub struct Connection(zbus::Connection);

impl Connection {
    /// Connects to the system bus.
    ///
    /// # Errors
    ///
    /// Returns [`OsError::Dbus`] if the connection fails.
    pub async fn system() -> Result<Self, OsError> {
        zbus::Connection::system()
            .await
            .map(Self)
            .map_err(OsError::from)
    }

    pub(crate) const fn inner(&self) -> &zbus::Connection {
        &self.0
    }
}
