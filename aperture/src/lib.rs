//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_http::tls::{
    RotateCertificateDefinition, SharedConfig, TlsListener, TlsReload, ensure_certificates,
    load_shared_config, redirect_router,
};
use aperture_http::{AppState, HttpServer, OpenApiSpec, Spectra, SpectraConfig};
use aperture_storage::{ListQuery, NewTaskSchedule, Storage};
use aperture_tasks::{Interval, Scheduler, TaskDefinition, TaskRegistry, Tasks};
use jiff::Timestamp;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use uuid::Uuid;

use self::runtime::{Supervisor, TasksWorker};

mod logging;
mod runtime;

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

    init_crypto_provider();

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

    ensure_certificates(&artifacts, https_addr).await?;
    install_default_rotation_schedule(&storage, https_addr).await?;
    scheduler.tick().await?;

    let shared_config: SharedConfig = load_shared_config(&artifacts).await?;

    let state = AppState::new(VERSION, boot_id, storage.clone(), spectra, tasks.clone());
    let app = aperture_http::app(state);

    let tcp_listener = TcpListener::bind(https_addr).await?;
    let tls_listener = TlsListener::new(tcp_listener, shared_config.clone());
    let tls_reload = TlsReload::new(Arc::clone(&artifacts), shared_config);
    tracing::info!(%https_addr, "aperture listening (https)");

    let mut server = HttpServer::new()
        .serve_tls(tls_listener, app.clone())
        .with_tls_reload(tls_reload);

    if let Some(http_addr) = http_addr {
        let http_listener = TcpListener::bind(http_addr).await?;
        if insecure_http {
            tracing::warn!(%http_addr, "serving full API over plain HTTP (insecure mode)");
            server = server.serve_http(http_listener, app.clone());
        } else {
            tracing::info!(%http_addr, "http redirect listening");
            let redirect = redirect_router(https_addr.port());
            server = server.serve_http(http_listener, redirect);
        }
    }

    let mut supervisor = Supervisor::new();
    supervisor.spawn("http", server);
    supervisor.spawn("tasks", TasksWorker::new(scheduler, tasks.clone()));
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
        .any(|s| s.kind == RotateCertificateDefinition::KIND);
    if already {
        return Ok(());
    }
    let now = Timestamp::now();
    repo.create(&NewTaskSchedule {
        kind: RotateCertificateDefinition::KIND.to_owned(),
        input: serde_json::json!({ "bind_addr": bind_addr.to_string() }),
        interval: Interval::from_micros(CERT_ROTATION_INTERVAL)
            .map_err(|e| anyhow::format_err!("invalid interval: {e}"))?,
        next_run_at: now,
        created_at: now,
    })
    .await?;
    Ok(())
}

/// Installs the `ring` crypto provider as the process-wide default.
fn init_crypto_provider() {
    use rustls::crypto::ring;
    let _ = ring::default_provider().install_default();
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
    registry.register(RotateCertificateDefinition::new(artifacts));
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
    let artifacts = Artifacts::new(storage.clone(), data_dir.join("store"));
    Ok((Arc::new(artifacts), storage))
}
