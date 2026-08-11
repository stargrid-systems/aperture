//! systemd-hostnamed integration: reading and applying the system hostname.

use aperture_tasks::{Capabilities, RunError, TaskContext, TaskDefinition};
use serde::{Deserialize, Serialize};
use zbus::Connection;
use zbus::proxy;
use utoipa::ToSchema;

use crate::error::OsError;

#[proxy(
    interface = "org.freedesktop.hostname1",
    default_service = "org.freedesktop.hostname1",
    default_path = "/org/freedesktop/hostname1"
)]
trait Hostname1 {
    fn set_hostname(&self, name: &str, user_interaction: bool) -> zbus::Result<()>;

    #[zbus(property)]
    fn hostname(&self) -> zbus::Result<String>;
}

/// Returns the current static hostname from systemd-hostnamed.
///
/// # Errors
///
/// Returns [`OsError::Dbus`] if the D-Bus call fails.
pub async fn read_hostname(connection: &Connection) -> Result<String, OsError> {
    let proxy = Hostname1Proxy::new(connection).await?;
    Ok(proxy.hostname().await?)
}

/// Applies `name` as the static hostname via systemd-hostnamed.
///
/// # Errors
///
/// Returns [`OsError::Dbus`] if the D-Bus call fails.
pub async fn apply_hostname(connection: &Connection, name: &str) -> Result<(), OsError> {
    let proxy = Hostname1Proxy::new(connection).await?;
    proxy.set_hostname(name, false).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReadHostnameInput {}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReadHostnameOutput {
    pub hostname: String,
}

pub struct ReadHostnameDefinition {
    connection: Connection,
}

impl ReadHostnameDefinition {
    pub const fn new(connection: Connection) -> Self {
        Self { connection }
    }
}

impl TaskDefinition for ReadHostnameDefinition {
    const KIND: &'static str = "read-hostname";
    type Input = ReadHostnameInput;
    type Output = ReadHostnameOutput;

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancellable: false,
            resumable: true,
        }
    }

    async fn run(
        &self,
        _input: ReadHostnameInput,
        _ctx: TaskContext,
    ) -> Result<ReadHostnameOutput, RunError> {
        let hostname = read_hostname(&self.connection)
            .await
            .map_err(|e| RunError::Failed(e.into()))?;
        Ok(ReadHostnameOutput { hostname })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApplyHostnameInput {
    pub hostname: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ApplyHostnameOutput {}

pub struct ApplyHostnameDefinition {
    connection: Connection,
}

impl ApplyHostnameDefinition {
    pub const fn new(connection: Connection) -> Self {
        Self { connection }
    }
}

impl TaskDefinition for ApplyHostnameDefinition {
    const KIND: &'static str = "apply-hostname";
    type Input = ApplyHostnameInput;
    type Output = ApplyHostnameOutput;

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancellable: false,
            resumable: true,
        }
    }

    async fn run(
        &self,
        input: ApplyHostnameInput,
        _ctx: TaskContext,
    ) -> Result<ApplyHostnameOutput, RunError> {
        apply_hostname(&self.connection, &input.hostname)
            .await
            .map_err(|e| RunError::Failed(e.into()))?;
        Ok(ApplyHostnameOutput {})
    }
}

#[cfg(test)]
mod tests {
    use aperture_settings::SettingDefinition;

    use super::*;
    use crate::setting::HostnameDef;

    #[test]
    fn read_hostname_kind() {
        assert_eq!(
            <ReadHostnameDefinition as TaskDefinition>::KIND,
            "read-hostname"
        );
    }

    #[test]
    fn apply_hostname_kind() {
        assert_eq!(
            <ApplyHostnameDefinition as TaskDefinition>::KIND,
            "apply-hostname"
        );
    }

    #[test]
    fn hostname_setting_key() {
        assert_eq!(HostnameDef::KEY, "hostname");
    }
}
