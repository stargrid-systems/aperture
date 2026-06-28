//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{Artifacts, DownloadDefinition};
pub use aperture_http::openapi;
use aperture_http::{AppState, Spectra, SpectraConfig};
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
    registry.register(DownloadDefinition::new(Arc::clone(&artifacts)));
    let tasks = TaskManager::new(artifacts.storage().clone(), registry);
    tasks.reconcile().await.into_diagnostic()?;

    let spectra = Spectra::new(Arc::clone(&artifacts), tasks.clone(), SpectraConfig::default());
    // Open a cached frontend right away. A missing one is fetched lazily on the
    // first request.
    spectra
        .activate_if_present()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;

    let state = AppState::new(VERSION, spectra, tasks);
    let app = aperture_http::app(state);

    let listener = TcpListener::bind(addr).await.into_diagnostic()?;
    tracing::info!(%addr, "aperture listening");
    axum::serve(listener, app).await.into_diagnostic()?;
    Ok(())
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
