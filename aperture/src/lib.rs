//! Aperture gateway: composes the HTTP layer with the artifact manager.
//!
//! # Features
//!
//! ## `os-integration`
//!
//! Opt-in deep OS integration, compiled in with this feature and enabled at
//! runtime with the `--os-integration` flag (or the
//! `APERTURE_OS_INTEGRATION` environment variable). When enabled, the
//! gateway:
//!
//! - publishes its listeners on the local network via mDNS/Avahi (`_https._tcp`
//!   and, without TLS, `_http._tcp`), including re-publication after hostname
//!   changes or Avahi restarts
//! - applies the configured hostname via systemd-hostnamed
//! - bakes a `<hostname>.local` SAN into its TLS certificate
//!
//! Requires a system D-Bus with `org.freedesktop.hostname1`
//! (systemd-hostnamed) and, for mDNS, `org.freedesktop.Avahi`
//! (avahi-daemon).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{
    ArtifactOrphanRemoved, ArtifactRemoved, ArtifactWritten, Artifacts, DownloadDefinition,
};
use aperture_auth::AuthHandle;
#[cfg(feature = "os-integration")]
use aperture_events::EventDefinition as _;
use aperture_events::{EventBus, EventRecorder, EventRegistry};
use aperture_http::{
    AppState, AvatarAnimation, AvatarStyle, HttpServer, RegenerateCertificateDefinition,
    RotateCertificateDefinition, Spectra, SpectraConfig, SpectraWorker,
    install_default_rotation_schedule,
};
use aperture_runtime::{ShutdownOutcome, Supervisor};
use aperture_settings::{SettingChange, SettingRegistry, Settings};
use aperture_storage::{ActorId, Storage};
#[cfg(feature = "os-integration")]
use aperture_tasks::TaskDefinition as _;
use aperture_tasks::{Automation, TaskRegistry, Tasks};
#[cfg(feature = "os-integration")]
use serde_json::json;
use tokio::sync::Mutex;
use tokio::{fs, signal};
use uuid::Uuid;

mod logging;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Installs `ring` as the process-wide rustls crypto provider.
///
/// The provider drives cipher suite selection, so it is never overridden: if
/// one is already installed (the serve tests run `serve` concurrently), that
/// one stays.
fn init_crypto_provider() {
    use rustls::crypto::ring;
    let _ = ring::default_provider().install_default();
}

/// Runs the gateway until the process is terminated.
///
/// `tls_addr` and `plain_addr` are independently optional. When both are
/// set, HTTP redirects to HTTPS. The TLS PKI and rotation schedule are only
/// touched when `tls_addr` is set. At least one listener must be set.
///
/// # Errors
///
/// Returns an error if neither listener is set, if both bind the same
/// address, if storage or artifact initialization fails, if a worker exits
/// early or hangs during shutdown, or if the server encounters a runtime
/// error.
#[expect(clippy::too_many_lines)]
pub async fn serve(
    tls_addr: Option<SocketAddr>,
    plain_addr: Option<SocketAddr>,
    data_dir: PathBuf,
    os_integration: bool,
) -> anyhow::Result<()> {
    if tls_addr.is_none() && plain_addr.is_none() {
        anyhow::bail!(
            "at least one of --https-addr or --http-addr must be set (pass an empty string to \
             disable a single listener)"
        );
    }
    if let (Some(a), Some(b)) = (tls_addr, plain_addr)
        && a == b
    {
        anyhow::bail!("--https-addr and --http-addr must differ (both were {a})");
    }

    let deferred_log_worker = logging::init();

    init_crypto_provider();
    let boot_id = Uuid::new_v4();
    let storage = open_storage(&data_dir).await?;
    let event_bus = EventBus::new();
    let artifacts = Artifacts::new(storage.clone(), data_dir.join("store"), event_bus.clone());

    let log_worker = deferred_log_worker.connect(storage.logs()?, boot_id);

    // Every fallible step from here on runs with a repository connected to
    // the log worker. If one fails, the workers started so far are drained
    // inline so the buffered startup records reach the database before the
    // error escapes. On success everything keeps running on the supervisor.
    let mut supervisor = Supervisor::new();
    let started = async {
        // The recorder must be connected and draining before `sync` runs:
        // sync emits one removal event per orphan version or blob, and
        // `emit` blocks once the recorder queue is full with no receiver
        // draining it.
        let Some(event_recorder) = EventRecorder::connect(&event_bus, storage.events()?) else {
            anyhow::bail!("event recorder connected twice");
        };
        supervisor.spawn_last("events", event_recorder);

        artifacts.sync().await?;

        // Auth: permission grants live in code, so the handle only rebuilds
        // the role index from storage.
        let auth = AuthHandle::new(storage.clone()).await?;

        let mut task_registry = TaskRegistry::new();
        let mut setting_registry = SettingRegistry::new();
        let mut event_registry = EventRegistry::new();

        #[cfg(feature = "os-integration")]
        let os_reg = if os_integration {
            Some(
                aperture_os::register(
                    &mut task_registry,
                    &mut setting_registry,
                    &mut event_registry,
                )
                .await?,
            )
        } else {
            None
        };

        #[cfg(not(feature = "os-integration"))]
        let _ = os_integration;

        register_tasks(&mut task_registry, artifacts.clone(), tls_addr);
        register_settings(&mut setting_registry);
        register_events(&mut event_registry);

        let tasks = Tasks::new(storage.tasks()?, task_registry);
        let settings = Settings::new(storage.settings()?, setting_registry, event_bus.clone());
        let automation = Automation::new(tasks.clone(), storage.task_schedules()?, &event_bus);

        #[cfg(feature = "os-integration")]
        let automation = {
            let mut automation = automation;
            if tls_addr.is_some() {
                automation.on_event(
                    aperture_os::HostnameApplied::KEY,
                    RegenerateCertificateDefinition::KEY,
                    |data| json!({ "hostname": data["hostname"] }),
                );
            }
            automation
        };

        let spectra = Spectra::new(
            artifacts.clone(),
            tasks.clone(),
            SpectraConfig::default(),
            ActorId::SYSTEM,
        );
        spectra.activate_if_present().await?;

        if tls_addr.is_some() {
            install_default_rotation_schedule(&storage).await?;
        }

        #[cfg(feature = "os-integration")]
        let (hostname, os) = match os_reg {
            Some(reg) => {
                let (hostname, os) =
                    aperture_os::bootstrap(reg, &settings, &tasks, event_bus.clone()).await?;
                (Some(hostname), Some(os))
            }
            None => (None, None),
        };

        #[cfg(not(feature = "os-integration"))]
        let hostname: Option<String> = None;

        let state = AppState::new(
            VERSION,
            boot_id,
            storage.clone(),
            spectra.clone(),
            tasks.clone(),
            settings,
            event_registry,
            auth,
        );
        let app = aperture_http::app(state);

        let server = HttpServer::start(
            artifacts,
            tls_addr,
            plain_addr,
            hostname.as_deref(),
            app,
            &event_bus,
        )
        .await?;

        // Read the real bound ports before the server is moved into the
        // supervisor, so mDNS advertises effective ports instead of
        // configured ones (which may be 0 for bind-any-port).
        #[cfg(feature = "os-integration")]
        let bound_ports = (server.tls_port(), server.plain_port());

        supervisor.spawn("http", server);
        supervisor.spawn("automation", automation);
        supervisor.spawn("spectra", SpectraWorker::new(spectra.clone()));

        #[cfg(feature = "os-integration")]
        if let Some(os) = os {
            let (tls_port, plain_port) = bound_ports;
            supervisor.spawn(
                "os",
                os.into_worker(tls_port, plain_port, tls_addr.is_some()),
            );
        }

        anyhow::Ok(())
    }
    .await;

    if let Err(err) = started {
        // Drain the workers started so far (the event recorder) so the
        // events queued during startup, including destructive sync removals,
        // are persisted instead of dropped with the process. The log worker
        // drains afterwards so it still records the recorder's flush.
        supervisor.shutdown().await;
        log_worker.drain().await;
        return Err(err);
    }

    supervisor.spawn_final("log", log_worker);

    let outcome = supervisor.run_until_signal(shutdown_signal()).await;
    match outcome {
        ShutdownOutcome::Signaled => Ok(()),
        ShutdownOutcome::Forced => {
            anyhow::bail!("forced exit: a worker hung during the shutdown drain")
        }
        ShutdownOutcome::EarlyExit { worker } => {
            anyhow::bail!("worker {worker} exited early")
        }
    }
}

/// Resolves when the process is asked to stop via Ctrl+C or SIGTERM.
async fn shutdown_signal() {
    let interrupt = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

/// Registers every task definition the gateway supports.
fn register_tasks(registry: &mut TaskRegistry, artifacts: Artifacts, tls_addr: Option<SocketAddr>) {
    let cert_lock = Arc::new(Mutex::new(()));

    registry.register(Arc::new(DownloadDefinition::new(artifacts.clone())));
    registry.register(Arc::new(RotateCertificateDefinition::new(
        artifacts.clone(),
        cert_lock.clone(),
    )));

    if let Some(addr) = tls_addr {
        registry.register(Arc::new(RegenerateCertificateDefinition::new(
            artifacts, addr, cert_lock,
        )));
    }
}

/// Registers every setting the gateway supports.
fn register_settings(registry: &mut SettingRegistry) {
    registry.register(Arc::new(AvatarStyle::default()));
    registry.register(Arc::new(AvatarAnimation::default()));
}

/// Registers every event kind the gateway emits.
fn register_events(registry: &mut EventRegistry) {
    registry.register(Arc::new(SettingChange::default()));
    registry.register(Arc::new(ArtifactWritten::default()));
    registry.register(Arc::new(ArtifactRemoved::default()));
    registry.register(Arc::new(ArtifactOrphanRemoved::default()));
}

/// Resets the password for `username` and prints the new password to stdout.
///
/// Revokes every active session for the user's actor so the old password
/// stops working immediately.
///
/// # Errors
///
/// Returns an error if storage cannot be opened, the user is not found, or
/// the password update fails.
pub async fn reset_password(username: &str, data_dir: &Path) -> anyhow::Result<()> {
    let db_path = data_dir.join("aperture.db");
    let db_path = db_path.to_str().ok_or_else(|| {
        anyhow::format_err!("data dir is not valid UTF-8: {}", data_dir.display())
    })?;
    let storage = Storage::open(db_path).await?;
    let users = storage.users()?;
    let user = users
        .find_by_username(username)
        .await?
        .ok_or_else(|| anyhow::format_err!("user {username:?} not found"))?;
    let password = aperture_auth::Password::generate();
    let hash = password.hash()?;
    users
        .update_password(user.id, &hash, Some(jiff::Timestamp::now()))
        .await?;
    storage.sessions()?.delete_for_actor(user.actor_id).await?;
    println!("{}", password.as_str());
    Ok(())
}

/// Opens the storage database under `data_dir`.
async fn open_storage(data_dir: &Path) -> anyhow::Result<Storage> {
    fs::create_dir_all(data_dir).await?;
    let db_path = data_dir.join("aperture.db");
    let db_path = db_path.to_str().ok_or_else(|| {
        anyhow::format_err!("data dir is not valid UTF-8: {}", data_dir.display())
    })?;
    Ok(Storage::open(db_path).await?)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aperture_events::EventDefinition;
    use aperture_storage::{EventFilter, ListQuery, LogEventFilter, Storage};

    use super::*;

    #[test]
    fn registers_every_emitted_event_kind() {
        let mut registry = EventRegistry::new();
        register_events(&mut registry);

        let mut registered: Vec<_> = registry.keys().collect();
        registered.sort_unstable();

        let mut emitted = [
            ArtifactOrphanRemoved::KEY,
            ArtifactRemoved::KEY,
            ArtifactWritten::KEY,
            SettingChange::KEY,
        ];
        emitted.sort_unstable();

        assert_eq!(registered, emitted);
    }

    #[tokio::test]
    async fn serve_rejects_when_no_listeners_configured() {
        let err = serve(None, None, PathBuf::from("/nonexistent"), false)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("at least one"),
            "expected a no-listeners error, got: {err}"
        );
    }

    /// A failed post-connect startup step must still persist the buffered
    /// log records. The port is occupied so `HttpServer::start`, the last
    /// startup step, fails after the log worker's repository is connected.
    #[tokio::test]
    async fn failed_startup_persists_buffered_logs() {
        use std::env::temp_dir;
        use std::net::TcpListener;
        use std::process::id;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let dir = temp_dir().join(format!("aperture-failed-startup-{}", id()));
        let _ = fs::remove_dir_all(&dir).await;
        let result = serve(None, Some(addr), dir.clone(), false).await;
        assert!(
            result.is_err(),
            "startup must fail while the port is occupied"
        );

        let db_path = dir.join("aperture.db");
        let storage = Storage::open(db_path.to_str().unwrap()).await.unwrap();
        let page = storage
            .logs()
            .unwrap()
            .list_events(&LogEventFilter::default(), &ListQuery::default())
            .await
            .unwrap();
        assert!(
            !page.items.is_empty(),
            "buffered startup logs must be persisted when startup fails"
        );

        drop(listener);
        let _ = fs::remove_dir_all(&dir).await;
    }

    /// A failed startup must persist the events queued during startup, so
    /// the destructive sync removals stay auditable. The data dir starts
    /// with more orphan blobs than the recorder queue can hold: with no
    /// recorder draining, boot would deadlock once the queue fills. The
    /// port is occupied so startup fails after sync, and every queued
    /// removal must still reach the database.
    #[tokio::test]
    async fn failed_startup_persists_queued_events() {
        use std::env::temp_dir;
        use std::net::TcpListener;
        use std::process::id;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let dir = temp_dir().join(format!("aperture-failed-startup-events-{}", id()));
        let _ = fs::remove_dir_all(&dir).await;
        let blobs = dir.join("store").join("blobs").join("sha256");
        fs::create_dir_all(&blobs).await.unwrap();
        for i in 0..1_100u32 {
            fs::write(blobs.join(format!("{i:064x}")), b"orphan")
                .await
                .unwrap();
        }

        let result = serve(None, Some(addr), dir.clone(), false).await;
        assert!(
            result.is_err(),
            "startup must fail while the port is occupied"
        );

        let db_path = dir.join("aperture.db");
        let storage = Storage::open(db_path.to_str().unwrap()).await.unwrap();
        let events = storage.events().unwrap();
        let mut query = ListQuery {
            limit: Some(200),
            ..Default::default()
        };
        let mut total = 0;
        loop {
            let page = events.list(&EventFilter::default(), &query).await.unwrap();
            total += page.items.len();
            let Some(cursor) = page.next_cursor else {
                break;
            };
            query.cursor = Some(cursor);
        }
        assert_eq!(total, 1_100, "every queued removal must be persisted");

        drop(listener);
        let _ = fs::remove_dir_all(&dir).await;
    }
}
