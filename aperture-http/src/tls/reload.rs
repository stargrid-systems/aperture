//! Hot-reload of TLS certificates via the artifact change feed.
//!
//! Reloads when `tls_server-cert` or `tls_server-key` is written or removed.
//! Writes within a short debounce window are coalesced. A failed reload is
//! retried a few times so transient races self-heal. The previous config
//! keeps serving until a reload succeeds.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_artifacts::{ArtifactChange, Artifacts, ChangeKind};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use super::error::TlsError;
use super::pki::reload_certificates;
use super::{SERVER_CERT, SERVER_KEY, SharedConfig};

/// Coalesces bursts of changes before a reload fires.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);
/// Backoff between reload attempts after a failure.
const RELOAD_RETRY_BACKOFF: Duration = Duration::from_secs(5);
/// Max reload attempts before giving up until the next change arrives.
const MAX_RELOAD_RETRIES: u32 = 6;

/// Watches the artifact change feed and hot-swaps the TLS server config.
pub struct TlsReload {
    artifacts: Artifacts,
    config: SharedConfig,
    rx: Receiver<ArtifactChange>,
}

impl TlsReload {
    pub fn new(artifacts: Artifacts, config: SharedConfig) -> Self {
        let rx = artifacts.subscribe();
        Self {
            artifacts,
            config,
            rx,
        }
    }

    /// Runs until `token` is cancelled.
    pub async fn run(mut self, token: CancellationToken) {
        let mut deadline: Option<Instant> = None;
        let mut retries: u32 = 0;
        loop {
            if let Some(when) = deadline {
                let sleep = sleep_until(when);
                tokio::pin!(sleep);
                tokio::select! {
                    biased;
                    _ = token.cancelled() => return,
                    recv = self.rx.recv() => {
                        if !apply_change(recv, &mut deadline, &mut retries) { return; }
                    }
                    _ = &mut sleep => {
                        match reload_certificates(&self.artifacts, &self.config).await {
                            Ok(()) => {
                                retries = 0;
                                deadline = None;
                            }
                            Err(err) => retry_after_failure(err, &mut deadline, &mut retries),
                        }
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => return,
                    recv = self.rx.recv() => {
                        if !apply_change(recv, &mut deadline, &mut retries) { return; }
                    }
                }
            }
        }
    }
}

/// Schedules a debounced reload when a relevant artifact changes (or the
/// watcher lagged). Returns false when the feed has closed.
fn apply_change(
    change: Result<ArtifactChange, RecvError>,
    deadline: &mut Option<Instant>,
    retries: &mut u32,
) -> bool {
    match change {
        Ok(ArtifactChange {
            key,
            kind: ChangeKind::Written | ChangeKind::Removed,
            digest,
        }) if key == SERVER_CERT || key == SERVER_KEY => {
            tracing::debug!(%key, ?digest, "scheduling TLS reload");
            *retries = 0;
            *deadline = Some(Instant::now() + RELOAD_DEBOUNCE);
        }
        Ok(_) => {}
        Err(RecvError::Lagged(n)) => {
            tracing::warn!(lag = n, "tls reload watcher lagged, scheduling reload");
            *retries = 0;
            *deadline = Some(Instant::now() + RELOAD_DEBOUNCE);
        }
        Err(RecvError::Closed) => {
            tracing::warn!("artifact change feed closed, TLS reload watcher exiting");
            return false;
        }
    }
    true
}

/// Schedules another reload attempt after a failure, or gives up and waits
/// for the next change. The previous config keeps serving throughout.
fn retry_after_failure(err: TlsError, deadline: &mut Option<Instant>, retries: &mut u32) {
    if *retries < MAX_RELOAD_RETRIES {
        *retries += 1;
        tracing::warn!(
            error = &err as &dyn StdError,
            attempt = *retries,
            "TLS reload failed, retrying in {RELOAD_RETRY_BACKOFF:?}"
        );
        *deadline = Some(Instant::now() + RELOAD_RETRY_BACKOFF);
    } else {
        tracing::error!(
            error = &err as &dyn StdError,
            "TLS reload failed after {MAX_RELOAD_RETRIES} retries; keeping current config until \
             the next change"
        );
        *retries = 0;
        *deadline = None;
    }
}
