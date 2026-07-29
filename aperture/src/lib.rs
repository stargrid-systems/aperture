//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::{
    AppState, HttpServer, OpenApiSpec, RotateCertificateDefinition, Spectra, SpectraConfig,
    SpectraWorker, init_crypto_provider, install_default_rotation_schedule,
};
use aperture_runtime::Supervisor;
use aperture_storage::Storage;
use aperture_tasks::{Scheduler, TaskRegistry, Tasks};
use tokio::{fs, signal};
use uuid::Uuid;

use self::runtime::TasksWorker;

mod logging;
mod runtime;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the gateway until the process is terminated.
///
/// `https_addr` and `http_addr` are independently optional:
///
/// - both set: HTTP listener redirects to HTTPS (default gateway setup)
/// - https only: HTTPS serves the full API
/// - http only: plain HTTP serves the full API (recovery mode)
/// - neither: the supervisor runs with no listeners
///
/// The TLS PKI and certificate rotation schedule are only touched when
/// `https_addr` is set. Both are idempotent, so disabling HTTPS leaves
/// any previously generated artifacts intact.
pub async fn serve(
    https_addr: Option<SocketAddr>,
    http_addr: Option<SocketAddr>,
    data_dir: PathBuf,
) -> anyhow::Result<()> {
    if matches!((https_addr, http_addr), (Some(a), Some(b)) if a == b) {
        let addr = https_addr.unwrap();
        anyhow::bail!("--https-addr and --http-addr must differ (both were {addr})");
    }

    let deferred_log_worker = logging::init();

    init_crypto_provider();
    let boot_id = Uuid::new_v4();
    let (artifacts, storage) = open_artifacts(&data_dir).await?;

    let log_worker = deferred_log_worker.connect(storage.logs()?, boot_id);

    artifacts.sync().await?;

    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, artifacts.clone());
    let tasks = Tasks::new(storage.tasks()?, registry);

    let scheduler = Scheduler::new(storage.task_schedules()?, tasks.clone());

    let spectra = Spectra::new(artifacts.clone(), tasks.clone(), SpectraConfig::default());
    spectra.activate_if_present().await?;

    if https_addr.is_some() {
        install_default_rotation_schedule(&storage).await?;
    }

    let state = AppState::new(
        VERSION,
        boot_id,
        storage.clone(),
        spectra.clone(),
        tasks.clone(),
    );
    let app = aperture_http::app(state);

    let server = HttpServer::start(artifacts, https_addr, http_addr, app).await?;

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
        _ = interrupt => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

/// Returns the OpenAPI specification, with the task kinds projected in.
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
