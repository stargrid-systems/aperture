//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::error::Error as StdError;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::{AppState, OpenApiSpec, Spectra, SpectraConfig};
use aperture_storage::{ListQuery, NewTaskSchedule, Storage};
use aperture_tasks::{Interval, Scheduler, TaskDefinition, TaskRegistry, Tasks};
use jiff::Timestamp;
use rustls::crypto::ring;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use uuid::Uuid;

use self::runtime::{HttpWorker, Supervisor, TasksWorker, TlsHttpWorker};

mod logging;
mod runtime;
mod tls;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default rotation interval: 24 hours in microseconds.
const CERT_ROTATION_INTERVAL: i64 = 24 * 60 * 60 * 1_000_000;

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
) -> anyhow::Result<()> {
    if http_addr == Some(https_addr) {
        anyhow::bail!("--https-addr and --http-addr must differ (both were {https_addr})");
    }

    ring::default_provider()
        .install_default()
        .expect("failed to install crypto provider");

    let deferred_log_worker = logging::init();

    let boot_id = Uuid::new_v4();
    let (artifacts, storage) = open_artifacts(&data_dir).await?;

    let log_worker = deferred_log_worker.connect(storage.logs()?, boot_id);

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

    tls::ensure_certificates(&artifacts, https_addr).await?;
    install_default_rotation_schedule(&storage, https_addr).await?;
    scheduler.tick().await?;

    let initial_config = tls::load_server_config(&artifacts).await?;
    let shared_config: tls::SharedConfig =
        Arc::new(arc_swap::ArcSwap::from_pointee(initial_config));

    let state = AppState::new(VERSION, boot_id, storage.clone(), spectra, tasks.clone());
    let app = aperture_http::app(state);

    let tcp_listener = TcpListener::bind(https_addr).await?;
    let tls_listener = tls::TlsListener::new(tcp_listener, shared_config.clone());
    tracing::info!(%https_addr, "aperture listening (https)");

    let mut supervisor = Supervisor::new();
    supervisor.spawn("https", TlsHttpWorker::new(tls_listener, app.clone()));

    if let Some(http_addr) = http_addr {
        let http_listener = TcpListener::bind(http_addr).await?;
        if insecure_http {
            tracing::warn!(%http_addr, "serving full API over plain HTTP (insecure mode)");
            supervisor.spawn("http", HttpWorker::new(http_listener, app.clone()));
        } else {
            tracing::info!(%http_addr, "http redirect listening");
            let redirect = tls::redirect_router(https_addr.port());
            supervisor.spawn("http", HttpWorker::new(http_listener, redirect));
        }
    }

    supervisor.spawn("tasks", TasksWorker::new(scheduler, tasks.clone()));
    supervisor.spawn(
        "tls-reload",
        TlsReloadWorker::new(Arc::clone(&artifacts), shared_config),
    );
    supervisor.spawn("log", log_worker);

    supervisor.run_until_signal(shutdown_signal()).await;

    tasks.shutdown().await;
    tracing::info!("aperture shutdown complete");

    Ok(())
}

/// Installs the default rotation schedule if no `rotate-certificate` schedule
/// exists.
async fn install_default_rotation_schedule(
    storage: &Storage,
    bind_addr: SocketAddr,
) -> anyhow::Result<()> {
    let repo = storage.task_schedules()?;
    let existing = repo.list(&ListQuery::default()).await?;
    let already = existing
        .items
        .iter()
        .any(|s| s.kind == tls::RotateCertificateDefinition::KIND);
    if already {
        return Ok(());
    }
    let now = Timestamp::now();
    repo.create(&NewTaskSchedule {
        kind: tls::RotateCertificateDefinition::KIND.to_owned(),
        input: serde_json::json!({ "bind_addr": bind_addr.to_string() }),
        interval: Interval::from_micros(CERT_ROTATION_INTERVAL)
            .map_err(|e| anyhow::anyhow!("invalid interval: {e}"))?,
        next_run_at: now,
        created_at: now,
    })
    .await?;
    Ok(())
}

/// Window over which multiple artifact writes are coalesced into a single
/// reload attempt.
const TLS_RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

struct TlsReloadWorker {
    artifacts: Arc<Artifacts>,
    config: tls::SharedConfig,
}

impl TlsReloadWorker {
    fn new(artifacts: Arc<Artifacts>, config: tls::SharedConfig) -> Self {
        Self { artifacts, config }
    }
}

impl runtime::Worker for TlsReloadWorker {
    async fn run(self, stop: runtime::Stop) {
        use aperture_artifacts::well_known::tls::{SERVER_CERT, SERVER_KEY};
        use aperture_artifacts::{ArtifactChange, ChangeKind};
        use tokio::sync::broadcast::error::RecvError;
        use tokio::time::{Instant, sleep_until};

        fn handle_change(
            change: Result<ArtifactChange, RecvError>,
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
                Err(RecvError::Lagged(_)) => {
                    tracing::warn!("tls reload watcher lagged the artifact feed");
                }
                Err(RecvError::Closed) => return false,
            }
            true
        }

        let mut rx = self.artifacts.subscribe();
        let mut deadline: Option<Instant> = None;
        let mut stop = stop;
        loop {
            if let Some(when) = deadline {
                let sleep = sleep_until(when);
                tokio::pin!(sleep);
                tokio::select! {
                    biased;
                    () = stop.as_mut() => return,
                    recv = rx.recv() => {
                        if !handle_change(recv, &mut deadline) { return; }
                    }
                    _ = &mut sleep => {
                        deadline = None;
                        tracing::info!("TLS reload requested");
                        if let Err(err) = tls::reload_certificates(&self.artifacts, &self.config).await {
                            tracing::error!(error = &err as &dyn StdError, "TLS reload failed");
                        }
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    () = stop.as_mut() => return,
                    recv = rx.recv() => {
                        if !handle_change(recv, &mut deadline) { return; }
                    }
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
pub async fn openapi() -> anyhow::Result<OpenApiSpec> {
    let storage = Storage::open(":memory:").await?;
    let artifacts = Artifacts::new(storage, PathBuf::from("."));
    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::new(artifacts));
    Ok(aperture_http::openapi(&registry.descriptors()))
}

/// Registers every task kind the gateway supports.
fn register_kinds(registry: &mut TaskRegistry, artifacts: Arc<Artifacts>) {
    registry.register(DownloadDefinition::new(Arc::clone(&artifacts)));
    registry.register(tls::RotateCertificateDefinition::new(artifacts));
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
