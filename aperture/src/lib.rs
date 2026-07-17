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
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod logging;
mod tls;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the gateway HTTPS server until the process is terminated.
///
/// `https_addr` is the HTTPS listener. `http_addr`, if given, starts a second
/// listener that either redirects to HTTPS (default) or serves the full API
/// over plain HTTP when `insecure_http` is set (recovery mode).
pub async fn serve(
    https_addr: SocketAddr,
    http_addr: Option<SocketAddr>,
    insecure_http: bool,
    data_dir: PathBuf,
) -> miette::Result<()> {
    if http_addr == Some(https_addr) {
        return Err(miette::miette!(
            "--https-addr and --http-addr must differ (both were {https_addr})"
        ));
    }

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

    tls::ensure_certificates(&artifacts, https_addr)
        .await
        .into_diagnostic()?;

    // Check at boot: if the existing leaf is already past half-life (or
    // expired), rotate before binding the listener so we never serve an
    // expired cert while waiting for the daily rotation tick.
    if tls::needs_rotation(&artifacts).await.into_diagnostic()? {
        tracing::info!("server certificate needs rotation at boot; rotating");
        tls::rotate_certificate(&artifacts, https_addr)
            .await
            .into_diagnostic()?;
    }

    let initial_config = tls::load_server_config(&artifacts)
        .await
        .into_diagnostic()?;
    let shared_config: tls::SharedConfig =
        Arc::new(arc_swap::ArcSwap::from_pointee(initial_config));

    // One token drives every shutdown path. The OS-signal handler task (in
    // `bg`) cancels it on Ctrl+C or SIGTERM; every long-running task and
    // every axum listener watches it so they all start their drain together.
    let shutdown = CancellationToken::new();
    let mut bg: JoinSet<()> = JoinSet::new();

    // Install the OS-signal watcher. On Ctrl+C or SIGTERM it cancels the
    // shared token, which simultaneously triggers graceful drain on every
    // axum listener and signals the loops below to return.
    {
        let shutdown = shutdown.clone();
        bg.spawn(async move {
            shutdown_signal().await;
            shutdown.cancel();
        });
    }

    // Live certificate reload. We subscribe to artifact changes and reload
    // whenever the server cert or key is written. A short debounce coalesces
    // back-to-back uploads of the cert and key halves (which arrive as two
    // separate writes) into one reload attempt; a failed reload (e.g. a
    // mismatched cert+key mid-upload) is logged and retried on the next event.
    bg.spawn(tls_reload_watcher(
        Arc::clone(&artifacts),
        shared_config.clone(),
        shutdown.clone(),
    ));

    // Periodic certificate rotation. Coalesced into the scheduler in a later
    // commit; for now the legacy loop stays.
    bg.spawn(rotation_loop(
        Arc::clone(&artifacts),
        https_addr,
        shutdown.clone(),
    ));

    let state = AppState::new(VERSION, boot_id, spectra, tasks.clone());
    let app = aperture_http::app(state);

    if let Some(http_addr) = http_addr {
        let http_listener = TcpListener::bind(http_addr).await.into_diagnostic()?;
        if insecure_http {
            tracing::warn!(%http_addr, "serving full API over plain HTTP (insecure mode)");
            let http_app = app.clone();
            let drain = {
                let token = shutdown.clone();
                async move { token.cancelled().await }
            };
            bg.spawn(async move {
                if let Err(err) = axum::serve(http_listener, http_app)
                    .with_graceful_shutdown(drain)
                    .await
                {
                    tracing::error!(error = &err as &dyn StdError, "http server failed");
                }
            });
        } else {
            tracing::info!(%http_addr, "http redirect listening");
            let redirect = tls::redirect_router(https_addr.port());
            let drain = {
                let token = shutdown.clone();
                async move { token.cancelled().await }
            };
            bg.spawn(async move {
                if let Err(err) = axum::serve(http_listener, redirect)
                    .with_graceful_shutdown(drain)
                    .await
                {
                    tracing::error!(error = &err as &dyn StdError, "http redirect failed");
                }
            });
        }
    }

    let tcp_listener = TcpListener::bind(https_addr).await.into_diagnostic()?;
    let tls_listener = tls::TlsListener::new(tcp_listener, shared_config);
    tracing::info!(%https_addr, "aperture listening (https)");
    let drain = {
        let token = shutdown.clone();
        async move { token.cancelled().await }
    };
    let result = axum::serve(tls_listener, app)
        .with_graceful_shutdown(drain)
        .await
        .into_diagnostic();

    tracing::info!("aperture shutdown starting");
    // Drain the background tasks. The signal handler returns as soon as it
    // has fired the token; the axum listeners finish their graceful drain;
    // the loops bail out via the cancelled token.
    while let Some(res) = bg.join_next().await {
        if let Err(err) = res
            && !err.is_cancelled()
        {
            tracing::error!(error = %err, "background task panicked");
        }
    }
    tasks.shutdown().await;
    tracing::info!("aperture shutdown complete");

    log_worker.shutdown().await;

    result?;
    Ok(())
}

/// Checks the server certificate daily and regenerates it at half-life. Reload
/// of the live TLS listener is triggered by the artifact change feed, so this
/// loop only writes the new cert.
async fn rotation_loop(
    artifacts: Arc<Artifacts>,
    bind_addr: SocketAddr,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => return,
            () = sleep(Duration::from_secs(24 * 60 * 60)) => {}
        }
        match tls::needs_rotation(&artifacts).await {
            Ok(true) => {
                tracing::info!("rotating server certificate");
                if let Err(err) = tls::rotate_certificate(&artifacts, bind_addr).await {
                    tracing::error!(error = &err as &dyn StdError, "certificate rotation failed");
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

/// Window over which multiple artifact writes are coalesced into a single
/// reload attempt. Long enough that a cert+key pair uploaded as two separate
/// requests collapses into one reload.
const TLS_RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

/// Subscribes to artifact changes and reloads the live TLS config whenever the
/// server certificate or key is written. Reload failures (e.g. a mismatched
/// cert+key mid-upload) are logged; the next qualifying event retries.
async fn tls_reload_watcher(
    artifacts: Arc<Artifacts>,
    config: tls::SharedConfig,
    shutdown: CancellationToken,
) {
    use aperture_artifacts::well_known::tls::{SERVER_CERT, SERVER_KEY};
    use aperture_artifacts::{ArtifactChange, ChangeKind};
    use tokio::time::Instant;

    fn handle_change(
        change: Result<ArtifactChange, tokio::sync::broadcast::error::RecvError>,
        deadline: &mut Option<Instant>,
    ) -> bool {
        match change {
            Ok(ArtifactChange {
                key,
                kind: ChangeKind::Written,
            }) if key == *SERVER_CERT || key == *SERVER_KEY => {
                let now = Instant::now();
                *deadline = Some(deadline.map_or(now, |d| d.max(now)) + TLS_RELOAD_DEBOUNCE);
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                tracing::warn!("tls reload watcher lagged the artifact feed");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
        }
        true
    }

    let mut rx = artifacts.subscribe();
    let mut deadline: Option<Instant> = None;
    loop {
        if let Some(when) = deadline {
            let sleep = tokio::time::sleep_until(when);
            tokio::pin!(sleep);
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return,
                recv = rx.recv() => {
                    if !handle_change(recv, &mut deadline) { return; }
                }
                _ = &mut sleep => {
                    deadline = None;
                    tracing::info!("TLS reload requested");
                    if let Err(err) = tls::reload_certificates(&artifacts, &config).await {
                        tracing::error!(error = &err as &dyn StdError, "TLS reload failed");
                    }
                }
            }
        } else {
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return,
                recv = rx.recv() => {
                    if !handle_change(recv, &mut deadline) { return; }
                }
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
