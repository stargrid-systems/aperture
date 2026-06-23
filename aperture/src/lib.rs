//! Aperture gateway: composes the core service with the HTTP layer.

use std::net::SocketAddr;
use std::path::PathBuf;

use aperture_core::Core;
use aperture_http::AppState;
pub use aperture_http::openapi;
use miette::IntoDiagnostic;

/// Pre-downloads components (the Spectra frontend) into `data_dir` for offline
/// use.
pub async fn prefetch(data_dir: PathBuf) -> miette::Result<()> {
    aperture_http::prefetch_spectra(data_dir).await
}

/// Runs the gateway HTTP server until the process is terminated.
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> miette::Result<()> {
    let state = AppState::new(Core::new());
    let app = aperture_http::app(state, data_dir);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .into_diagnostic()?;
    tracing::info!(%addr, "aperture listening");
    axum::serve(listener, app).await.into_diagnostic()?;
    Ok(())
}
