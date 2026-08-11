//! systemd-hostnamed integration: applying the system hostname.

use anyhow::Context;
use aperture_tasks::{Capabilities, RunError, TaskContext, TaskDefinition};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zbus::proxy;

use crate::setting::Hostname;

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

async fn apply_hostname(connection: &zbus::Connection, name: &str) -> anyhow::Result<()> {
    let proxy = Hostname1Proxy::new(connection)
        .await
        .context("failed to create hostname1 proxy")?;
    proxy
        .set_hostname(name, false)
        .await
        .context("failed to set static hostname")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApplyHostnameInput {
    pub hostname: Hostname,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ApplyHostnameOutput {}

pub struct ApplyHostnameDefinition {
    connection: zbus::Connection,
}

impl ApplyHostnameDefinition {
    pub const fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

impl TaskDefinition for ApplyHostnameDefinition {
    const KIND: &'static str = "apply-hostname";
    type Input = ApplyHostnameInput;
    type Output = ApplyHostnameOutput;

    fn capabilities(&self) -> Capabilities {
        Capabilities::NONE
    }

    async fn run(
        &self,
        input: ApplyHostnameInput,
        _ctx: TaskContext,
    ) -> Result<ApplyHostnameOutput, RunError> {
        apply_hostname(&self.connection, input.hostname.as_str())
            .await
            .map_err(RunError::Failed)?;
        Ok(ApplyHostnameOutput {})
    }
}
