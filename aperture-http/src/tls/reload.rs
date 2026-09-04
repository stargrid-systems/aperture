//! Hot-reload of TLS certificates via domain events.
//!
//! Reloads when `tls_server-cert` or `tls_server-key` is written or removed.
//! Writes within a short debounce window are coalesced. A failed reload is
//! retried a few times so transient races self-heal. The previous config
//! keeps serving until a reload succeeds.
//!
//! Orphan-blob removals (`artifact.orphan-removed`) deliberately do not
//! reload: an orphan blob has no catalog entry, so it cannot be the material
//! the reload loads by key.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_artifacts::{ArtifactRemoved, ArtifactWritten, Artifacts};
use aperture_events::{Delivery, EventBus, TypedEvent, TypedEventStream};
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use super::pki::reload_certificates;
use super::{SERVER_CERT, SERVER_KEY, SharedConfig};

/// Coalesces bursts of changes before a reload fires.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);
/// Backoff between reload attempts after a failure.
const RELOAD_RETRY_BACKOFF: Duration = Duration::from_secs(5);
/// Max reload attempts before giving up until the next change arrives.
const MAX_RELOAD_RETRIES: u32 = 6;

/// Watches artifact events and hot-swaps the TLS server config.
pub struct TlsReload {
    artifacts: Artifacts,
    config: SharedConfig,
    written: TypedEventStream<ArtifactWritten>,
    removed: TypedEventStream<ArtifactRemoved>,
}

impl TlsReload {
    pub fn new(artifacts: Artifacts, config: SharedConfig, event_bus: &EventBus) -> Self {
        Self {
            artifacts,
            config,
            written: event_bus.subscribe_typed::<ArtifactWritten>(),
            removed: event_bus.subscribe_typed::<ArtifactRemoved>(),
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
                    () = token.cancelled() => return,
                    Some(delivery) = self.written.recv() => {
                        handle_change(delivery, &mut deadline, &mut retries);
                    }
                    Some(delivery) = self.removed.recv() => {
                        handle_change(delivery, &mut deadline, &mut retries);
                    }
                    () = &mut sleep => {
                        match reload_certificates(&self.artifacts, &self.config).await {
                            Ok(()) => {
                                retries = 0;
                                deadline = None;
                            }
                            Err(err) => {
                                if retries < MAX_RELOAD_RETRIES {
                                    retries += 1;
                                    tracing::warn!(
                                        error = &err as &dyn StdError,
                                        attempt = retries,
                                        "TLS reload failed, retrying in {RELOAD_RETRY_BACKOFF:?}"
                                    );
                                    deadline = Some(Instant::now() + RELOAD_RETRY_BACKOFF);
                                } else {
                                    tracing::error!(
                                        error = &err as &dyn StdError,
                                        "TLS reload failed after {MAX_RELOAD_RETRIES} retries; \
                                         keeping current config until the next change"
                                    );
                                    retries = 0;
                                    deadline = None;
                                }
                            }
                        }
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    () = token.cancelled() => return,
                    Some(delivery) = self.written.recv() => {
                        handle_change(delivery, &mut deadline, &mut retries);
                    }
                    Some(delivery) = self.removed.recv() => {
                        handle_change(delivery, &mut deadline, &mut retries);
                    }
                }
            }
        }
    }
}

/// Payloads of artifact events that name the artifact key they changed.
trait ArtifactChange {
    fn artifact_key(&self) -> &str;
}

impl ArtifactChange for ArtifactWritten {
    fn artifact_key(&self) -> &str {
        &self.key
    }
}

impl ArtifactChange for ArtifactRemoved {
    fn artifact_key(&self) -> &str {
        &self.key
    }
}

/// Applies one artifact event delivery. A lag report means relevant
/// changes may have been missed, so a reload is scheduled unconditionally.
fn handle_change<D: ArtifactChange>(
    delivery: Delivery<TypedEvent<D>>,
    deadline: &mut Option<Instant>,
    retries: &mut u32,
) {
    match delivery {
        Delivery::Event(event) => {
            check_artifact_key(event.payload.artifact_key(), deadline, retries);
        }
        Delivery::Lagged(dropped) => {
            tracing::warn!(dropped, "TLS event stream lagged, scheduling reload");
            *retries = 0;
            *deadline = Some(Instant::now() + RELOAD_DEBOUNCE);
        }
    }
}

/// Schedules a debounced reload when a relevant artifact changes.
fn check_artifact_key(key: &str, deadline: &mut Option<Instant>, retries: &mut u32) {
    if key == SERVER_CERT.as_str() || key == SERVER_KEY.as_str() {
        tracing::debug!(%key, "scheduling TLS reload");
        *retries = 0;
        *deadline = Some(Instant::now() + RELOAD_DEBOUNCE);
    }
}
