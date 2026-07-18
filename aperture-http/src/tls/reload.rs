//! Hot-reload of TLS certificates via the artifact change feed.
//!
//! [`TlsReload`] subscribes to artifact writes and reloads the server
//! certificate when `tls/server-cert` or `tls/server-key` changes. Multiple
//! writes within a short debounce window are coalesced into a single reload.

use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;

use aperture_artifacts::well_known::tls::{SERVER_CERT, SERVER_KEY};
use aperture_artifacts::{ArtifactChange, Artifacts, ChangeKind};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use super::SharedConfig;
use super::pki::reload_certificates;

/// Window over which multiple artifact writes are coalesced into a single
/// reload attempt.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

/// Watches the artifact change feed and hot-swaps the TLS server config when
/// the certificate or key changes.
pub struct TlsReload {
    artifacts: Arc<Artifacts>,
    config: SharedConfig,
}

impl TlsReload {
    /// Creates a reload watcher that swaps `config` on certificate changes.
    pub fn new(artifacts: Arc<Artifacts>, config: SharedConfig) -> Self {
        Self { artifacts, config }
    }

    /// Runs the watcher until `token` is cancelled.
    pub async fn run(self, token: CancellationToken) {
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
                    *deadline = Some(deadline.map_or(now, |d| d.max(now)) + RELOAD_DEBOUNCE);
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
        loop {
            if let Some(when) = deadline {
                let sleep = sleep_until(when);
                tokio::pin!(sleep);
                tokio::select! {
                    biased;
                    _ = token.cancelled() => return,
                    recv = rx.recv() => {
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
                    recv = rx.recv() => {
                        if !handle_change(recv, &mut deadline) { return; }
                    }
                }
            }
        }
    }
}
