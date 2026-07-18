//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::{AppState, OpenApiSpec, Spectra, SpectraConfig};
use aperture_storage::Storage;
use aperture_tasks::{Scheduler, TaskRegistry, Tasks};
use miette::IntoDiagnostic;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use uuid::Uuid;

use self::runtime::{HttpWorker, Supervisor, TasksWorker};

mod logging;
mod runtime;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the gateway HTTP server until the process is terminated.
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> miette::Result<()> {
    let boot_id = Uuid::new_v4();

    // Install the tracing subscriber before anything else so startup is
    // captured. The DB side of the layer buffers to a channel; the worker is
    // attached once storage is open below.
    let deferred_log_worker = logging::init();

    let (artifacts, storage) = open_artifacts(&data_dir).await?;
    artifacts.sync().await.into_diagnostic()?;
    let log_worker = deferred_log_worker.connect(storage.logs().into_diagnostic()?, boot_id);

    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::clone(&artifacts));
    let tasks = Tasks::new(storage.tasks().into_diagnostic()?, registry);
    tasks.reconcile().await.into_diagnostic()?;

    let scheduler = Scheduler::new(storage.task_schedules().into_diagnostic()?, tasks.clone());

    let spectra = Spectra::new(
        Arc::clone(&artifacts),
        tasks.clone(),
        SpectraConfig::default(),
    );
    spectra
        .activate_if_present()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;

    let state = AppState::new(VERSION, boot_id, storage, spectra, tasks.clone());
    let app = aperture_http::app(state);

    let listener = TcpListener::bind(addr).await.into_diagnostic()?;
    tracing::info!(%addr, "aperture listening");

    // Drain order is registration order: HTTP stops accepting first, then the
    // scheduler stops spawning and in-flight tasks drain, then the log worker
    // flushes last so shutdown logs land in the database.
    let mut supervisor = Supervisor::new();
    supervisor.spawn("http", HttpWorker::new(listener, app));
    supervisor.spawn("tasks", TasksWorker::new(scheduler, tasks.clone()));
    supervisor.spawn("log", log_worker);

    supervisor.run_until_signal(shutdown_signal()).await;
    Ok(())
}

/// Resolves when the process is asked to stop, via Ctrl+C or SIGTERM.
async fn shutdown_signal() {
    let interrupt = async {
        ctrl_c().await.expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};

        signal(SignalKind::terminate())
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
pub async fn openapi() -> miette::Result<OpenApiSpec> {
    let storage = Storage::open(":memory:").await.into_diagnostic()?;
    let artifacts = Artifacts::new(storage, PathBuf::from("."));
    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::new(artifacts));
    Ok(aperture_http::openapi(&registry.descriptors()))
}

/// Registers every task kind the gateway supports.
fn register_kinds(registry: &mut TaskRegistry, artifacts: Arc<Artifacts>) {
    registry.register(DownloadDefinition::new(artifacts));
}

/// Opens the storage database and blob store under `data_dir`. Returns the
/// artifact manager and the storage handle so callers can build their own
/// repositories alongside it.
async fn open_artifacts(data_dir: &Path) -> miette::Result<(Arc<Artifacts>, Storage)> {
    fs::create_dir_all(data_dir).await.into_diagnostic()?;
    let db_path = data_dir.join("aperture.db");
    let db_path = db_path
        .to_str()
        .ok_or_else(|| miette::miette!("data dir is not valid UTF-8: {}", data_dir.display()))?;
    let storage = Storage::open(db_path).await.into_diagnostic()?;
    let artifacts = Arc::new(Artifacts::new(storage.clone(), data_dir.join("store")));
    Ok((artifacts, storage))
}
