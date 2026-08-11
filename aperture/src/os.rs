//! OS integration: mDNS publishing, hostname management, and task wiring.
//!
//! The module is always compiled. When the `os-integration` Cargo feature is
//! not enabled, the public functions are no-ops. When enabled but the runtime
//! flag is off, they return empty results.

use std::net::SocketAddr;

#[cfg(feature = "os-integration")]
use self::worker::OsWorker;

#[cfg(feature = "os-integration")]
mod worker;

/// Holds the D-Bus connection created during task registration.
pub struct OsRegistration {
    #[cfg(feature = "os-integration")]
    pub(crate) conn: aperture_os::Connection,
}

/// Result of OS integration bootstrap.
pub struct OsBoot {
    /// The resolved hostname for TLS SAN provisioning, or `None` when OS
    /// integration is inactive.
    pub hostname: Option<String>,
    #[cfg(feature = "os-integration")]
    worker: Option<OsWorker>,
}

#[cfg(not(feature = "os-integration"))]
pub async fn register_tasks(
    _os_integration: bool,
    _registry: &mut aperture_tasks::TaskRegistry,
) -> anyhow::Result<Option<OsRegistration>> {
    Ok(None)
}

#[cfg(feature = "os-integration")]
pub async fn register_tasks(
    os_integration: bool,
    registry: &mut aperture_tasks::TaskRegistry,
) -> anyhow::Result<Option<OsRegistration>> {
    if !os_integration {
        return Ok(None);
    }
    let conn = aperture_os::Connection::system()
        .await
        .map_err(|e| anyhow::format_err!("failed to connect to system D-Bus: {e}"))?;
    registry.register(aperture_os::ReadHostnameDefinition::new(conn.clone()));
    registry.register(aperture_os::ApplyHostnameDefinition::new(conn.clone()));
    Ok(Some(OsRegistration { conn }))
}

#[cfg(not(feature = "os-integration"))]
pub async fn bootstrap(
    _registration: Option<OsRegistration>,
    _settings: aperture_settings::Settings,
    _tasks: aperture_tasks::Tasks,
    _tls_addr: Option<SocketAddr>,
    _plain_addr: Option<SocketAddr>,
) -> anyhow::Result<OsBoot> {
    Ok(OsBoot { hostname: None })
}

#[cfg(feature = "os-integration")]
pub async fn bootstrap(
    registration: Option<OsRegistration>,
    settings: aperture_settings::Settings,
    tasks: aperture_tasks::Tasks,
    tls_addr: Option<SocketAddr>,
    plain_addr: Option<SocketAddr>,
) -> anyhow::Result<OsBoot> {
    let Some(reg) = registration else {
        return Ok(OsBoot {
            hostname: None,
            worker: None,
        });
    };

    let hostname = resolve_hostname(&settings, &tasks).await?;
    let worker = OsWorker::new(
        settings.clone(),
        tasks.clone(),
        reg.conn,
        hostname.clone(),
        tls_addr.map(|a| a.port()),
        plain_addr.map(|a| a.port()),
    );
    Ok(OsBoot {
        hostname: Some(hostname),
        worker: Some(worker),
    })
}

impl OsBoot {
    /// Spawns the OS worker into `supervisor` if one was created.
    pub fn spawn(self, supervisor: &mut aperture_runtime::Supervisor) {
        #[cfg(feature = "os-integration")]
        if let Some(worker) = self.worker {
            supervisor.spawn("os", worker);
        }

        #[cfg(not(feature = "os-integration"))]
        {
            let _ = (self, supervisor);
        }
    }
}

#[cfg(feature = "os-integration")]
async fn resolve_hostname(
    settings: &aperture_settings::Settings,
    tasks: &aperture_tasks::Tasks,
) -> anyhow::Result<String> {
    use aperture_storage::ActorId;

    let value: aperture_os::Hostname = settings.get::<aperture_os::HostnameDef>().await?;
    if let Some(name) = value.as_str() {
        let handle = tasks
            .spawn::<aperture_os::ApplyHostnameDefinition>(
                aperture_os::ApplyHostnameInput {
                    hostname: name.to_owned(),
                },
                ActorId::SYSTEM,
            )
            .await?;
        handle.wait().await?;
        Ok(name.to_owned())
    } else {
        let handle = tasks
            .spawn::<aperture_os::ReadHostnameDefinition>(
                aperture_os::ReadHostnameInput {},
                ActorId::SYSTEM,
            )
            .await?;
        let output = handle.wait().await?;
        Ok(output.hostname)
    }
}
