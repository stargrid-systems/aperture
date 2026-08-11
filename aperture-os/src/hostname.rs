//! systemd-hostnamed integration: applying the system hostname.

use aperture_tasks::{Capabilities, RunError, TaskContext, TaskDefinition};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zbus::proxy;

use crate::Connection;
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

/// Applies `name` as the static hostname via systemd-hostnamed.
///
/// # Errors
///
/// Returns [`OsError::Dbus`] if the D-Bus call fails.
pub async fn apply_hostname(connection: &Connection, name: &str) -> Result<(), OsError> {
    let proxy = Hostname1Proxy::new(connection.inner()).await?;
    proxy.set_hostname(name, false).await?;
    Ok(())
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
