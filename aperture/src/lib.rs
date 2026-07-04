//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::error::Error as StdError;

use aperture_artifacts::Artifacts;
pub use aperture_http::openapi;
use aperture_http::{AppState, Spectra, SpectraConfig};
use miette::IntoDiagnostic;
use tokio::fs;
use tokio::net::TcpListener;

mod logging;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the gateway HTTP server until the process is terminated.
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> miette::Result<()> {
    let artifacts = open_artifacts(&data_dir).await?;
    let boot_id = uuid::Uuid::new_v4();
    let log_writer = artifacts
        .storage()
        .log_writer()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;
    let log_worker = logging::init(log_writer, boot_id);

    artifacts.sync().await.into_diagnostic()?;

    let spectra = Spectra::new(artifacts.clone(), SpectraConfig::default());
    // Open a cached frontend right away. A missing one is fetched lazily on the
    // first request.
    spectra
        .activate_if_present()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;

    let state = AppState::new(VERSION, boot_id, spectra);
    let app = aperture_http::app(state);

    let listener = TcpListener::bind(addr).await.into_diagnostic()?;
    tracing::info!(%addr, "aperture listening");
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .into_diagnostic();

    tracing::info!("aperture shutdown starting");
    tracing::info!("aperture shutdown complete");
    if let Ok(logs) = artifacts.storage().logs()
        && let Err(err) = logs.close_open_spans(jiff::Timestamp::now()).await
    {
        tracing::warn!(error = &err as &dyn StdError, "failed to close open spans on shutdown");
    }
    log_worker.shutdown().await;

    result?;
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

/// Resolves on Ctrl-C or SIGTERM, whichever arrives first.
async fn shutdown_signal() {
    use tokio::signal::ctrl_c;
    let ctrl_c = async {
        let _ = ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        let _ = signal(SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
