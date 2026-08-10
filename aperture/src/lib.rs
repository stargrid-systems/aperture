//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_auth::AuthHandle;
use aperture_http::{
    AppState, HttpServer, OpenApiSpec, RotateCertificateDefinition, Spectra, SpectraConfig,
    SpectraWorker, install_default_rotation_schedule,
};
use aperture_runtime::Supervisor;
use aperture_settings::{SettingRegistry, Settings};
use aperture_storage::{ActorId, Storage};
use aperture_tasks::{Scheduler, TaskRegistry, Tasks};
use tokio::{fs, signal};
use uuid::Uuid;

use self::runtime::TasksWorker;

mod logging;
mod runtime;

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
/// address, if storage or artifact initialization fails, or if the server
/// encounters a runtime error.
///
/// # Panics
///
/// Panics if the rustls crypto provider is already installed.
pub async fn serve(
    tls_addr: Option<SocketAddr>,
    plain_addr: Option<SocketAddr>,
    data_dir: PathBuf,
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
    let (artifacts, storage) = open_artifacts(&data_dir).await?;

    let log_worker = deferred_log_worker.connect(storage.logs()?, boot_id);

    artifacts.sync().await?;

    // Auth: build enforcer and seed default policies.
    let auth = AuthHandle::new(storage.clone()).await?;

    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, artifacts.clone());
    let tasks = Tasks::new(storage.tasks()?, registry);

    let mut setting_registry = SettingRegistry::new();
    register_settings(&mut setting_registry);
    let settings = Settings::new(storage.settings()?, setting_registry);

    let scheduler = Scheduler::new(storage.task_schedules()?, tasks.clone());

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
        auth,
    );
    let app = aperture_http::app(state);

    let server = HttpServer::start(artifacts, tls_addr, plain_addr, app).await?;

    let mut supervisor = Supervisor::new();
    supervisor.spawn("http", server);
    supervisor.spawn("tasks", TasksWorker::new(scheduler, tasks.clone()));
    supervisor.spawn("log", log_worker);
    supervisor.spawn("spectra", SpectraWorker::new(spectra.clone()));

    supervisor.run_until_signal(shutdown_signal()).await;
    Ok(())
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

/// Returns the `OpenAPI` specification, with the task kinds projected in.
///
/// # Errors
///
/// Returns an error if the in-memory storage cannot be opened.
pub async fn openapi() -> anyhow::Result<OpenApiSpec> {
    let storage = Storage::open(":memory:").await?;
    let artifacts = Artifacts::new(storage, PathBuf::from("."));
    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, artifacts);
    Ok(aperture_http::openapi(&registry.descriptors()))
}

/// Registers every task kind the gateway supports.
fn register_kinds(registry: &mut TaskRegistry, artifacts: Artifacts) {
    registry.register(DownloadDefinition::new(artifacts.clone()));
    registry.register(RotateCertificateDefinition::new(artifacts));
}

/// Registers every setting scope the gateway supports.
const fn register_settings(_registry: &mut SettingRegistry) {}

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

/// Opens the storage database and blob store under `data_dir`.
///
/// Returns the artifact manager and the storage handle so callers can build
/// their own repositories alongside it.
async fn open_artifacts(data_dir: &Path) -> anyhow::Result<(Artifacts, Storage)> {
    fs::create_dir_all(data_dir).await?;
    let db_path = data_dir.join("aperture.db");
    let db_path = db_path.to_str().ok_or_else(|| {
        anyhow::format_err!("data dir is not valid UTF-8: {}", data_dir.display())
    })?;
    let storage = Storage::open(db_path).await?;
    let artifacts = Artifacts::new(storage.clone(), data_dir.join("store"));
    Ok((artifacts, storage))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[tokio::test]
    async fn serve_rejects_when_no_listeners_configured() {
        let err = serve(None, None, PathBuf::from("/nonexistent"))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("at least one"),
            "expected a no-listeners error, got: {err}"
        );
    }
}
