//! OS integration for the Aperture gateway.
//!
//! Provides mDNS service publication via Avahi and hostname management via
//! systemd-hostnamed. This crate is an aperture module: it owns the full
//! OS integration lifecycle (D-Bus connection, worker loop, settings feed)
//! and exposes a simple register/bootstrap API.
//!
//! Events emitted during operation (e.g. hostname applied) are available via
//! [`OsWorker::subscribe`] for consumers that need to react to OS state changes.

use std::error::Error as StdError;

use anyhow::Context;
use aperture_settings::{SettingRegistry, Settings};
use aperture_tasks::{TaskRegistry, Tasks};

use self::hostname::{ApplyHostnameDefinition, ApplyHostnameInput};
use self::setting::HostnameSetting;

pub use self::error::HostnameError;
pub use self::setting::Hostname;
pub use self::worker::{OsEvent, OsWorker};

mod avahi;
mod error;
mod hostname;
mod setting;
mod worker;

/// Internal state between [`register`] and [`bootstrap`].
pub struct OsRegistration {
    conn: zbus::Connection,
}

/// Registers OS task and setting definitions.
///
/// Connects to the system D-Bus and registers the `apply-hostname` task kind
/// and the `os.hostname` setting.
///
/// # Errors
///
/// Returns an error if the D-Bus connection fails.
pub async fn register(
    task_registry: &mut TaskRegistry,
    setting_registry: &mut SettingRegistry,
) -> anyhow::Result<OsRegistration> {
    let conn = zbus::Connection::system()
        .await
        .context("failed to connect to system D-Bus")?;
    task_registry.register(ApplyHostnameDefinition::new(conn.clone()));
    setting_registry.register(HostnameSetting::default());
    Ok(OsRegistration { conn })
}

/// Bootstraps OS integration: applies the hostname and creates the worker.
///
/// Reads the configured hostname from settings and applies it via
/// systemd-hostnamed. If the apply fails, a warning is logged and startup
/// continues.
///
/// # Errors
///
/// Returns an error if the hostname setting cannot be read from storage.
pub async fn bootstrap(
    reg: OsRegistration,
    settings: &Settings,
    tasks: &Tasks,
    https_port: Option<u16>,
    plain_port: Option<u16>,
) -> anyhow::Result<(String, OsWorker)> {
    let setting: HostnameSetting = settings.get().await?;
    let hostname = setting.hostname().clone();
    let hostname_str = hostname.as_str().to_owned();

    match tasks
        .spawn::<ApplyHostnameDefinition>(
            ApplyHostnameInput {
                hostname: hostname.clone(),
            },
            aperture_storage::ActorId::SYSTEM,
        )
        .await
    {
        Ok(handle) => {
            if let Err(err) = handle.wait().await {
                tracing::warn!(
                    error = &err as &dyn StdError,
                    "initial hostname apply failed"
                );
            }
        }
        Err(err) => {
            tracing::warn!(
                error = &err as &dyn StdError,
                "failed to spawn hostname task"
            );
        }
    }

    let worker = OsWorker::new(
        settings.clone(),
        tasks.clone(),
        reg.conn,
        hostname_str.clone(),
        https_port,
        plain_port,
    );

    Ok((hostname_str, worker))
}
