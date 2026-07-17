//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::error::Error as StdError;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::{AppState, OpenApiSpec, Spectra, SpectraConfig};
use aperture_tasks::{TaskRegistry, Tasks};
use miette::IntoDiagnostic;
use rustls::crypto::ring;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use tokio::sync::watch;
use tokio::time::sleep;
use uuid::Uuid;

mod logging;
mod tls;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the gateway HTTPS server until the process is terminated.
///
/// `addr` is the HTTPS listener. `http_addr`, if given, starts a second
/// listener that either redirects to HTTPS (default) or serves the full API
/// over plain HTTP when `insecure_http` is set (recovery mode).
pub async fn serve(
    addr: SocketAddr,
    http_addr: Option<SocketAddr>,
    insecure_http: bool,
    data_dir: PathBuf,
) -> miette::Result<()> {
    ring::default_provider()
        .install_default()
        .expect("failed to install crypto provider");

    let artifacts = open_artifacts(&data_dir).await?;
    let boot_id = Uuid::new_v4();
    let log_repo = artifacts.storage().logs().into_diagnostic()?;
    let log_worker = logging::init(log_repo, boot_id);

    artifacts.sync().await.into_diagnostic()?;

    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::clone(&artifacts));
    let tasks = Tasks::new(artifacts.storage().clone(), registry);
    tasks.reconcile().await.into_diagnostic()?;

    let spectra = Spectra::new(
        Arc::clone(&artifacts),
        tasks.clone(),
        SpectraConfig::default(),
    );
    spectra
        .activate_if_present()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;

    tls::ensure_certificates(&artifacts, addr).await.into_diagnostic()?;
    let initial_config = tls::load_server_config(&artifacts).await.into_diagnostic()?;
    let shared_config = tls::shared_config(initial_config);

    let (tls_reload_tx, mut tls_reload_rx) = watch::channel(());
    {
        let artifacts = Arc::clone(&artifacts);
        let config = shared_config.clone();
        tokio::spawn(async move {
            while tls_reload_rx.changed().await.is_ok() {
                tracing::info!("TLS reload requested");
                if let Err(err) = tls::reload_certificates(&artifacts, &config).await {
                    tracing::error!(error = &err as &dyn StdError, "TLS reload failed");
                }
            }
        });
    }

    {
        let artifacts = Arc::clone(&artifacts);
        let reload_tx = tls_reload_tx.clone();
        tokio::spawn(rotation_loop(artifacts, addr, reload_tx));
    }

    let state = AppState::new(VERSION, boot_id, spectra, tasks.clone(), tls_reload_tx);
    let app = aperture_http::app(state);

    if let Some(http_addr) = http_addr {
        let http_listener = TcpListener::bind(http_addr).await.into_diagnostic()?;
        if insecure_http {
            tracing::warn!(%http_addr, "serving full API over plain HTTP (insecure mode)");
            let http_app = app.clone();
            tokio::spawn(async move {
                if let Err(err) = axum::serve(http_listener, http_app)
                    .with_graceful_shutdown(shutdown_signal())
                    .await
                {
                    tracing::error!(error = &err as &dyn StdError, "http server failed");
                }
            });
        } else {
            tracing::info!(%http_addr, "http redirect listening");
            let redirect = tls::redirect_router(addr.port());
            tokio::spawn(async move {
                if let Err(err) = axum::serve(http_listener, redirect)
                    .with_graceful_shutdown(shutdown_signal())
                    .await
                {
                    tracing::error!(error = &err as &dyn StdError, "http redirect failed");
                }
            });
        }
    }

    let tcp_listener = TcpListener::bind(addr).await.into_diagnostic()?;
    let tls_listener = tls::TlsListener::new(tcp_listener, shared_config);
    tracing::info!(%addr, "aperture listening (https)");
    let result = axum::serve(tls_listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .into_diagnostic();

    tracing::info!("aperture shutdown starting");
    tasks.shutdown().await;
    tracing::info!("aperture shutdown complete");

    log_worker.shutdown().await;

    result?;
    Ok(())
}

/// Checks the server certificate daily and regenerates it at half-life.
async fn rotation_loop(
    artifacts: Arc<Artifacts>,
    bind_addr: SocketAddr,
    reload_tx: watch::Sender<()>,
) {
    loop {
        sleep(Duration::from_secs(24 * 60 * 60)).await;
        match tls::needs_rotation(&artifacts).await {
            Ok(true) => {
                tracing::info!("rotating server certificate");
                match tls::rotate_certificate(&artifacts, bind_addr).await {
                    Ok(()) => {
                        let _ = reload_tx.send(());
                    }
                    Err(err) => {
                        tracing::error!(
                            error = &err as &dyn StdError,
                            "certificate rotation failed"
                        );
                    }
                }
            }
            Ok(false) => {}
            Err(err) => {
                tracing::error!(
                    error = &err as &dyn StdError,
                    "certificate rotation check failed"
                );
            }
        }
    }
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
