//! OS integration: mDNS publishing and hostname management.

use std::net::SocketAddr;

use aperture_os::Connection;
use aperture_settings::Settings;
use aperture_tasks::Tasks;

use self::worker::OsWorker;

mod worker;

/// Holds the D-Bus connection created during registration.
pub struct OsRegistration {
    conn: Connection,
}

/// Registers OS task and setting definitions when integration is active.
///
/// Returns `None` when `os_integration` is false.
pub async fn register(
    os_integration: bool,
    task_registry: &mut aperture_tasks::TaskRegistry,
    setting_registry: &mut aperture_settings::SettingRegistry,
) -> anyhow::Result<Option<OsRegistration>> {
    if !os_integration {
        return Ok(None);
    }
    let conn = Connection::system().await?;
    task_registry.register(aperture_os::ApplyHostnameDefinition::new(conn.clone()));
    setting_registry.register(aperture_os::HostnameDef);
    Ok(Some(OsRegistration { conn }))
}

/// Bootstraps OS integration: applies the hostname and creates the worker.
///
/// Returns `(None, None)` when integration was not registered.
pub async fn bootstrap(
    registration: Option<OsRegistration>,
    settings: &Settings,
    tasks: &Tasks,
    tls_addr: Option<SocketAddr>,
    plain_addr: Option<SocketAddr>,
) -> anyhow::Result<(Option<String>, Option<OsWorker>)> {
    let Some(reg) = registration else {
        return Ok((None, None));
    };

    let hostname: aperture_os::Hostname = settings.get::<aperture_os::HostnameDef>().await?;
    let hostname_str = hostname.as_str().to_owned();

    let handle = tasks
        .spawn::<aperture_os::ApplyHostnameDefinition>(
            aperture_os::ApplyHostnameInput {
                hostname: hostname_str.clone(),
            },
            aperture_storage::ActorId::SYSTEM,
        )
        .await?;
    handle.wait().await?;

    let worker = OsWorker::new(
        settings.clone(),
        tasks.clone(),
        reg.conn,
        hostname_str.clone(),
        tls_addr.map(|a| a.port()),
        plain_addr.map(|a| a.port()),
    );

    Ok((Some(hostname_str), Some(worker)))
}
