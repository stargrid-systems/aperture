//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::error::Error;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::{AppState, OpenApiSpec, Spectra, SpectraConfig};
use aperture_storage::Storage;
use aperture_tasks::{Scheduler, TaskRegistry, Tasks};
use miette::IntoDiagnostic;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod logging;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often the scheduler wakes to check for due schedules.
const SCHEDULER_TICK: Duration = Duration::from_secs(60);

/// Runs the gateway HTTP server until the process is terminated.
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> miette::Result<()> {
    let (artifacts, storage) = open_artifacts(&data_dir).await?;
    let boot_id = Uuid::new_v4();
    let log_repo = storage.logs().into_diagnostic()?;
    let log_worker = logging::init(log_repo, boot_id);

    artifacts.sync().await.into_diagnostic()?;

    // Register the task kinds and mark any invocations a previous run left
    // active as interrupted.
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
    // Open a cached frontend right away. A missing one is fetched lazily on the
    // first request.
    spectra
        .activate_if_present()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;

    let state = AppState::new(VERSION, boot_id, storage, spectra, tasks.clone());
    let app = aperture_http::app(state);

    let listener = TcpListener::bind(addr).await.into_diagnostic()?;
    tracing::info!(%addr, "aperture listening");

    // One token drives every shutdown path. The OS-signal handler cancels it,
    // which stops the scheduler driver and triggers axum's graceful drain.
    let shutdown = CancellationToken::new();

    // Periodic task scheduler: spawns due schedules on each tick. Cancelled on
    // shutdown so the driver returns during the drain. A boot tick runs first so
    // schedules that came due while the gateway was down fire right away instead
    // of waiting up to `SCHEDULER_TICK`.
    let scheduler_task = {
        let shutdown = shutdown.clone();
        let scheduler = scheduler.clone();
        tokio::spawn(async move {
            if let Err(err) = scheduler.tick().await {
                tracing::error!(error = &err as &dyn Error, "scheduler boot tick failed",);
            }
            scheduler.run(SCHEDULER_TICK, shutdown).await;
        })
    };

    let shutdown_signal_token = shutdown.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_signal_token.cancel();
        })
        .await
        .into_diagnostic();

    // Stop accepting new periodic work before draining the task manager.
    let _ = scheduler_task.await;

    // The server has stopped accepting requests.
    tracing::info!("aperture shutdown starting");
    tasks.shutdown().await;
    tracing::info!("aperture shutdown complete");

    // The log worker drains pending records, commits them, and closes any
    // spans left open. close_open_spans runs inside the worker so it sees the
    // final flush.
    log_worker.shutdown().await;

    result?;
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
