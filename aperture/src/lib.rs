//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{ArtifactRemoved, ArtifactWritten, Artifacts, DownloadDefinition};
use aperture_auth::AuthHandle;
use aperture_events::{EventBus, EventRecorder, EventRegistry};
use aperture_http::{
    AppState, AvatarAnimation, AvatarStyle, HttpServer, RegenerateCertificateDefinition,
    RotateCertificateDefinition, Spectra, SpectraConfig, SpectraWorker,
    install_default_rotation_schedule,
};
use aperture_runtime::{ShutdownOutcome, Supervisor};
use aperture_settings::{SettingChange, SettingRegistry, Settings};
use aperture_storage::{ActorId, Storage};
use aperture_tasks::{Automation, TaskDefinition, TaskRegistry, Tasks};
use serde_json::json;
use tokio::sync::Mutex;
use tokio::{fs, signal};
use uuid::Uuid;

mod logging;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Installs `ring` as the process-wide rustls crypto provider.
///
/// Panics if a provider is already installed. The provider drives cipher
/// suite selection, so silently overriding it could change the security
/// posture.
fn init_crypto_provider() {
    use rustls::crypto::ring;
    ring::default_provider()
        .install_default()
        .expect("crypto provider already installed");
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
///
/// # Panics
///
/// Panics if the rustls crypto provider is already installed.
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
    if matches!((tls_addr, plain_addr), (Some(a), Some(b)) if a == b) {
        let addr = tls_addr.unwrap();
        anyhow::bail!("--https-addr and --http-addr must differ (both were {addr})");
    }

    let deferred_log_worker = logging::init();

    init_crypto_provider();
    let boot_id = Uuid::new_v4();
    let storage = open_storage(&data_dir).await?;
    let event_bus = EventBus::new();
    let artifacts = Artifacts::new(storage.clone(), data_dir.join("store"), event_bus.clone());

    let log_worker = deferred_log_worker.connect(storage.logs()?, boot_id);

    // Every fallible step from here on runs with a repository connected to
    // the log worker. If one fails, the worker is drained inline so the
    // buffered startup records reach the database before the error escapes.
    // On success the worker is spawned on the supervisor below instead.
    let mut supervisor = match async {
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
        let mut automation = Automation::new(tasks.clone(), storage.task_schedules()?, &event_bus);

        if tls_addr.is_some() {
            automation.on_event(
                "os.hostname_applied",
                RegenerateCertificateDefinition::KEY,
                |data| json!({ "hostname": data["hostname"] }),
            );
        }

        #[cfg(feature = "os-integration")]
        let (hostname, os_worker) = match os_reg {
            Some(reg) => {
                let (h, w) = aperture_os::bootstrap(
                    reg,
                    &settings,
                    &tasks,
                    tls_addr.map(|a| a.port()),
                    plain_addr.map(|a| a.port()),
                    event_bus.clone(),
                )
                .await?;
                (Some(h), Some(w))
            }
            None => (None, None),
        };

        #[cfg(not(feature = "os-integration"))]
        let hostname: Option<String> = None;

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

        let Some(event_recorder) = EventRecorder::connect(&event_bus, storage.events()?) else {
            anyhow::bail!("event recorder connected twice");
        };

        let server = HttpServer::start(
            artifacts,
            tls_addr,
            plain_addr,
            hostname.as_deref(),
            app,
            &event_bus,
        )
        .await?;

        let mut supervisor = Supervisor::new();
        supervisor.spawn("http", server);
        supervisor.spawn("automation", automation);
        supervisor.spawn("spectra", SpectraWorker::new(spectra.clone()));
        supervisor.spawn_last("events", event_recorder);

        #[cfg(feature = "os-integration")]
        if let Some(worker) = os_worker {
            supervisor.spawn("os", worker);
        }

        anyhow::Ok(supervisor)
    }
    .await
    {
        Ok(supervisor) => supervisor,
        Err(err) => {
            log_worker.drain().await;
            return Err(err);
        }
    };
    supervisor.spawn_last("log", log_worker);

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
    use aperture_storage::{ListQuery, LogEventFilter, Storage};

    use super::*;

    #[test]
    fn registers_every_emitted_event_kind() {
        let mut registry = EventRegistry::new();
        register_events(&mut registry);

        let mut registered: Vec<_> = registry.keys().collect();
        registered.sort_unstable();

        let mut emitted = [
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
}
