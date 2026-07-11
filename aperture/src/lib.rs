//! Aperture gateway: composes the HTTP layer with the artifact manager.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::{Artifacts, DownloadDefinition};
use aperture_auth::AuthHandle;
use aperture_http::{AppState, OpenApiSpec, Spectra, SpectraConfig};
use aperture_tasks::{TaskRegistry, Tasks};
use miette::IntoDiagnostic;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use uuid::Uuid;

mod logging;

/// Version of the Aperture gateway.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the gateway HTTP server until the process is terminated.
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> miette::Result<()> {
    let artifacts = open_artifacts(&data_dir).await?;
    let boot_id = Uuid::new_v4();
    let log_repo = artifacts.storage().logs().into_diagnostic()?;
    let log_worker = logging::init(log_repo, boot_id);

    artifacts.sync().await.into_diagnostic()?;

    let storage = artifacts.storage().clone();

    // Auth: build enforcer, seed policies, ensure system actor, bootstrap
    // admin.
    let auth = AuthHandle::new(storage.clone())
        .await
        .map_err(|e| miette::miette!("{e:#}"))?;
    let system_actor = auth
        .ensure_system_actor()
        .await
        .map_err(|e| miette::miette!("{e:#}"))?;
    match auth.bootstrap_admin().await {
        Ok(Some(password)) => {
            eprintln!("--- Admin bootstrap ---");
            eprintln!("Username: admin");
            eprintln!("Password: {password}");
            eprintln!("------------------------");
        }
        Ok(None) => {}
        Err(e) => return Err(miette::miette!("{e:#}")),
    }

    // Register the task kinds and mark any invocations a previous run left
    // active as interrupted.
    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::clone(&artifacts));
    let tasks = Tasks::new(storage, registry);
    tasks.reconcile().await.into_diagnostic()?;

    let spectra = Spectra::new(
        Arc::clone(&artifacts),
        tasks.clone(),
        SpectraConfig::default(),
        system_actor,
    );
    // Open a cached frontend right away. A missing one is fetched lazily on the
    // first request.
    spectra
        .activate_if_present()
        .await
        .map_err(|error| miette::miette!("{error:#}"))?;

    let state = AppState::new(VERSION, boot_id, spectra, tasks.clone(), auth);
    let app = aperture_http::app(state);

    let listener = TcpListener::bind(addr).await.into_diagnostic()?;
    tracing::info!(%addr, "aperture listening");
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .into_diagnostic();

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
    let artifacts = Artifacts::open(":memory:", PathBuf::from("."))
        .await
        .into_diagnostic()?;
    let mut registry = TaskRegistry::new();
    register_kinds(&mut registry, Arc::new(artifacts));
    Ok(aperture_http::openapi(&registry.descriptors()))
}

/// Resets the password for `username`. Prints the new password to stdout.
pub async fn reset_password(username: &str, data_dir: &Path) -> miette::Result<()> {
    let db_path = data_dir.join("aperture.db");
    let db_path = db_path
        .to_str()
        .ok_or_else(|| miette::miette!("data dir is not valid UTF-8: {}", data_dir.display()))?;
    let storage = aperture_storage::Storage::open(db_path)
        .await
        .into_diagnostic()?;
    let users = storage.users().into_diagnostic()?;
    let user = users
        .find_by_username(username)
        .await
        .into_diagnostic()?
        .ok_or_else(|| miette::miette!("user {username:?} not found"))?;
    let password = aperture_auth::generate_session_token();
    let hash = aperture_auth::hash_password(&password).into_diagnostic()?;
    users
        .update_password(user.id, &hash, true)
        .await
        .into_diagnostic()?;
    println!("{password}");
    Ok(())
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
