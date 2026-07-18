//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::{AppState, OpenApiSpec, Spectra, SpectraConfig};
use aperture_storage::Storage;
use aperture_tasks::{Scheduler, TaskRegistry, Tasks};
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
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> anyhow::Result<()> {
    let deferred_log_worker = logging::init();

    let boot_id = Uuid::new_v4();
    let (artifacts, storage) = open_artifacts(&data_dir).await?;

    let log_worker = deferred_log_worker.connect(storage.logs()?, boot_id);

    // TODO: this should be handled through the task engine somehow.
    artifacts.sync().await?;

    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::clone(&artifacts));
    let tasks = Tasks::new(storage.tasks()?, registry);

    let scheduler = Scheduler::new(storage.task_schedules()?, tasks.clone());

    let spectra = Spectra::new(
        Arc::clone(&artifacts),
        tasks.clone(),
        SpectraConfig::default(),
    );
    spectra.activate_if_present().await?;

    let state = AppState::new(VERSION, boot_id, storage, spectra, tasks.clone());
    let app = aperture_http::app(state);

    // TODO: ideally this should be owned by the http worker, but we need to make it
    // fallible.
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "aperture listening");

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
pub async fn openapi() -> anyhow::Result<OpenApiSpec> {
    let storage = Storage::open(":memory:").await?;
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
async fn open_artifacts(data_dir: &Path) -> anyhow::Result<(Arc<Artifacts>, Storage)> {
    fs::create_dir_all(data_dir).await?;
    let db_path = data_dir.join("aperture.db");
    let db_path = db_path.to_str().ok_or_else(|| {
        anyhow::format_err!("data dir is not valid UTF-8: {}", data_dir.display())
    })?;
    let storage = Storage::open(db_path).await?;
    let artifacts = Arc::new(Artifacts::new(storage.clone(), data_dir.join("store")));
    Ok((artifacts, storage))
}
