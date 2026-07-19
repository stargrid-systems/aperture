//! Hot-reload of TLS certificates via the artifact change feed.
//!
//! [`TlsReload`] subscribes to artifact writes and reloads the server
//! certificate when `tls/server-cert` or `tls/server-key` changes. Multiple
//! writes within a short debounce window are coalesced into a single reload.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_artifacts::{ArtifactChange, Artifacts, ChangeKind};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use super::pki::reload_certificates;
use super::{SERVER_CERT, SERVER_KEY, SharedConfig};

/// Window over which multiple artifact writes are coalesced into a single
/// reload attempt.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

/// Watches the artifact change feed and hot-swaps the TLS server config when
/// the certificate or key changes.
///
/// The subscription is created in [`TlsReload::new`] so events emitted between
/// construction and [`TlsReload::run`] are buffered by the broadcast channel
/// and observed when `run` starts. Otherwise a rotation completing between
/// `HttpServer::start` returning and `run` reaching `subscribe` would be lost
/// for the lifetime of the process.
pub struct TlsReload {
    artifacts: Artifacts,
    config: SharedConfig,
    rx: Receiver<ArtifactChange>,
}

impl TlsReload {
    /// Creates a reload watcher that swaps `config` on certificate changes.
    pub fn new(artifacts: Artifacts, config: SharedConfig) -> Self {
        let rx = artifacts.subscribe();
        Self {
            artifacts,
            config,
            rx,
        }
    }

    /// Runs the watcher until `token` is cancelled.
    pub async fn run(mut self, token: CancellationToken) {
        let mut deadline: Option<Instant> = None;
        loop {
            if let Some(when) = deadline {
                let sleep = sleep_until(when);
                tokio::pin!(sleep);
                tokio::select! {
                    biased;
                    _ = token.cancelled() => return,
                    recv = self.rx.recv() => {
                        if !handle_change(recv, &mut deadline) { return; }
                    }
                    _ = &mut sleep => {
                        deadline = None;
                        tracing::info!("TLS reload requested");
                        if let Err(err) = reload_certificates(&self.artifacts, &self.config).await {
                            tracing::error!(error = &err as &dyn StdError, "TLS reload failed");
                        }
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => return,
                    recv = self.rx.recv() => {
                        if !handle_change(recv, &mut deadline) { return; }
                    }
                }
            }
        }
    }
}

/// Handles a single change-feed event.
///
/// Returns `false` when the feed has closed and the watcher should exit.
fn handle_change(
    change: Result<ArtifactChange, RecvError>,
    deadline: &mut Option<Instant>,
) -> bool {
    match change {
        Ok(ArtifactChange {
            key,
            kind: ChangeKind::Written,
        }) if key == SERVER_CERT || key == SERVER_KEY => {
            *deadline = Some(Instant::now() + RELOAD_DEBOUNCE);
        }
        Ok(_) => {}
        // A lagged receiver silently skipped an unknown number of messages.
        // Schedule a reload unconditionally because one of the dropped events
        // may have been a cert or key write. Without this, the watcher would
        // keep serving a stale cert (potentially already expired) until the
        // next unrelated change arrives.
        Err(RecvError::Lagged(n)) => {
            tracing::warn!(
                lag = n,
                "tls reload watcher lagged the artifact feed, scheduling a reload"
            );
            *deadline = Some(Instant::now() + RELOAD_DEBOUNCE);
        }
        Err(RecvError::Closed) => {
            tracing::warn!("artifact change feed closed, TLS reload watcher exiting");
            return false;
        }
    }
    true
}
