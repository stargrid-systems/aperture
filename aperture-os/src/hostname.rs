//! systemd-hostnamed integration: reading and applying the system hostname.

use zbus::Connection;
use zbus::proxy;

use crate::error::OsError;

#[proxy(
    interface = "org.freedesktop.hostname1",
    default_service = "org.freedesktop.hostname1",
    default_path = "/org/freedesktop/hostname1"
)]
trait Hostname1 {
    /// Sets the static hostname. `user_interaction` is false for programmatic
    /// requests.
    fn set_hostname(&self, name: &str, user_interaction: bool) -> zbus::Result<()>;

    /// The static hostname configured on the system.
    #[zbus(property)]
    fn hostname(&self) -> zbus::Result<String>;
}

/// Returns the current static hostname from systemd-hostnamed.
///
/// # Errors
///
/// Returns `OsError::Dbus` if the D-Bus call fails.
pub async fn read_hostname(connection: &Connection) -> Result<String, OsError> {
    let proxy = Hostname1Proxy::new(connection).await?;
    Ok(proxy.hostname().await?)
}

/// Applies `name` as the static hostname via systemd-hostnamed.
///
/// # Errors
///
/// Returns `OsError::Dbus` if the D-Bus call fails.
pub async fn apply_hostname(connection: &Connection, name: &str) -> Result<(), OsError> {
    let proxy = Hostname1Proxy::new(connection).await?;
    proxy.set_hostname(name, false).await?;
    Ok(())
}
