//! OS integration for the Aperture gateway.
//!
//! Provides mDNS service publication via Avahi and hostname management via
//! systemd-hostnamed. This crate is an aperture module: it owns the full
//! OS integration lifecycle (D-Bus connection, worker loop, settings feed)
//! and exposes a simple register/bootstrap API.
//!
//! Domain events (e.g. hostname applied) are emitted through the central
//! [`EventBus`].

use std::error::Error as StdError;
use std::sync::Arc;

use anyhow::Context;
use aperture_events::{EventBus, EventRegistry, TypedEventStream};
use aperture_settings::{SettingChange, SettingRegistry, Settings};
use aperture_tasks::{TaskRegistry, Tasks};

pub use self::error::HostnameError;
pub use self::event::HostnameApplied;
use self::hostname::{ApplyHostnameDefinition, ApplyHostnameInput};
pub use self::setting::Hostname;
use self::setting::HostnameSetting;
pub use self::worker::OsWorker;

mod avahi;
mod error;
mod event;
mod hostname;
mod setting;
mod worker;

/// Internal state between [`register`] and [`bootstrap`].
pub struct OsRegistration {
    conn: zbus::Connection,
}

/// Everything [`bootstrap`] produced and [`OsIntegration::into_worker`]
/// consumes.
pub struct OsIntegration {
    conn: zbus::Connection,
    hostname: String,
    tasks: Tasks,
    event_bus: EventBus,
    setting_changes: TypedEventStream<SettingChange>,
}

impl OsIntegration {
    /// Creates the background worker for the final listener layout.
    ///
    /// `https_port` and `plain_port` are the ports the listeners actually
    /// bound (never `0`). The plain listener is only advertised as
    /// `_http._tcp` when `tls_enabled` is false; with TLS it merely
    /// redirects.
    pub fn into_worker(
        self,
        https_port: Option<u16>,
        plain_port: Option<u16>,
        tls_enabled: bool,
    ) -> OsWorker {
        OsWorker::new(
            self.tasks,
            self.conn,
            self.hostname,
            https_port,
            plain_port,
            tls_enabled,
            self.event_bus,
            self.setting_changes,
        )
    }
}

/// Registers OS task, setting, and event definitions.
///
/// Connects to the system D-Bus and registers the `apply-hostname` task kind,
/// the `os.hostname` setting, and the `os.hostname_applied` event.
///
/// # Errors
///
/// Returns an error if the D-Bus connection fails.
pub async fn register(
    task_registry: &mut TaskRegistry,
    setting_registry: &mut SettingRegistry,
    event_registry: &mut EventRegistry,
) -> anyhow::Result<OsRegistration> {
    let conn = zbus::Connection::system()
        .await
        .context("failed to connect to system D-Bus")?;
    task_registry.register(Arc::new(ApplyHostnameDefinition::new(conn.clone())));
    setting_registry.register(Arc::new(HostnameSetting::default()));
    event_registry.register(Arc::new(HostnameApplied::default()));
    Ok(OsRegistration { conn })
}

/// Bootstraps OS integration: applies the hostname and prepares the worker.
///
/// Reads the configured hostname from settings and applies it via
/// systemd-hostnamed. If the apply fails, a warning is logged and startup
/// continues.
///
/// Returns the hostname plus the [`OsIntegration`] that builds the actual
/// worker once the listeners are bound:
///
/// ```no_run
/// # use aperture_events::EventBus;
/// # use aperture_settings::Settings;
/// # use aperture_tasks::Tasks;
/// # async fn example(
/// #     reg: aperture_os::OsRegistration,
/// #     settings: &Settings,
/// #     tasks: &Tasks,
/// #     event_bus: EventBus,
/// # ) -> anyhow::Result<()> {
/// let (hostname, os) = aperture_os::bootstrap(reg, settings, tasks, event_bus).await?;
/// let worker = os.into_worker(Some(8443), Some(8080), true);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns an error if the hostname setting cannot be read from storage.
pub async fn bootstrap(
    reg: OsRegistration,
    settings: &Settings,
    tasks: &Tasks,
    event_bus: EventBus,
) -> anyhow::Result<(String, OsIntegration)> {
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

    let setting_changes = event_bus.subscribe_typed::<SettingChange>();
    Ok((
        hostname_str.clone(),
        OsIntegration {
            conn: reg.conn,
            hostname: hostname_str,
            tasks: tasks.clone(),
            event_bus,
            setting_changes,
        },
    ))
}
