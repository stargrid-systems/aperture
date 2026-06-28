//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::{AppState, OpenApiSpec, Spectra, SpectraConfig};
use aperture_tasks::{TaskManager, TaskRegistry};
use miette::IntoDiagnostic;
use tokio::fs;
use tokio::net::TcpListener;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the gateway HTTP server until the process is terminated.
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> miette::Result<()> {
    let artifacts = open_artifacts(&data_dir).await?;
    artifacts.sync().await.into_diagnostic()?;

    // Register the task kinds and mark any invocations a previous run left
    // active as interrupted.
    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::clone(&artifacts));
    let tasks = TaskManager::new(artifacts.storage().clone(), registry);
    tasks.reconcile().await.into_diagnostic()?;

    let spectra = Spectra::new(Arc::clone(&artifacts), tasks.clone(), SpectraConfig::default());
    // Open a cached frontend right away. A missing one is fetched lazily on the
    // first request.
    spectra
        .activate_if_present()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;

    let state = AppState::new(VERSION, spectra, tasks.clone());
    let app = aperture_http::app(state);

    let listener = TcpListener::bind(addr).await.into_diagnostic()?;
    tracing::info!(%addr, "aperture listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .into_diagnostic()?;

    // The server has stopped accepting requests. Resolve the running tasks:
    // resumable ones are interrupted, unresumable ones are awaited.
    tracing::info!("draining tasks before exit");
    tasks.shutdown().await;
    Ok(())
}

/// Resolves when the process is asked to stop, via Ctrl+C or SIGTERM.
async fn shutdown_signal() {
    use tokio::signal::ctrl_c;

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
    let artifacts = Artifacts::open(":memory:", PathBuf::from("."))
        .await
        .into_diagnostic()?;
    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::new(artifacts));
    Ok(aperture_http::openapi(&registry.descriptors()))
}

/// Registers every task kind the gateway supports.
fn register_kinds(registry: &mut TaskRegistry, artifacts: Arc<Artifacts>) {
    registry.register(DownloadDefinition::new(artifacts));
}

/// Opens the storage database and blob store under `data_dir`.
async fn open_artifacts(data_dir: &Path) -> miette::Result<Arc<Artifacts>> {
    fs::create_dir_all(data_dir).await.into_diagnostic()?;
    let db_path = data_dir.join("aperture.db");
    let db_path = db_path
        .to_str()
        .ok_or_else(|| miette::miette!("data dir is not valid UTF-8: {}", data_dir.display()))?;
    let artifacts = Artifacts::open(db_path, data_dir.join("store"))
        .await
        .into_diagnostic()?;
    Ok(Arc::new(artifacts))
}
