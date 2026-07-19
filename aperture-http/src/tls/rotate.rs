//! The certificate-rotation task: re-issue the leaf cert when it nears expiry.
//!
//! Driven by the periodic scheduler. The task body only writes the new cert;
//! the live TLS listener picks up the change via the artifact change feed.

use std::net::SocketAddr;

use aperture_artifacts::Artifacts;
use aperture_storage::{ListQuery, NewTaskSchedule, Storage};
use aperture_tasks::{Capabilities, Interval, RunError, TaskContext, TaskDefinition};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::pki;

/// Default rotation interval: 24 hours in microseconds.
const ROTATION_INTERVAL_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// Input for the rotate-certificate task.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateCertificateInput {
    /// The address the HTTPS listener is bound to, formatted as `host:port`.
    ///
    /// The bind IP is added to the leaf's Subject Alternative Names when it
    /// differs from the localhost defaults.
    pub bind_addr: String,
}

impl RotateCertificateInput {
    /// Parses the bind address from its string form.
    pub fn parse_bind_addr(&self) -> Result<SocketAddr, RunError> {
        self.bind_addr
            .parse::<SocketAddr>()
            .map_err(|e| RunError::Failed(anyhow::Error::from(e)))
    }
}

/// Output of the rotate-certificate task.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateCertificateOutput {
    /// Whether the leaf was actually re-issued.
    ///
    /// `false` means the existing cert still had enough remaining validity and
    /// no rotation was needed.
    pub rotated: bool,
}

/// Task definition for periodic certificate rotation.
pub struct RotateCertificateDefinition {
    artifacts: Artifacts,
}

impl RotateCertificateDefinition {
    /// Creates the definition over `artifacts`.
    pub fn new(artifacts: Artifacts) -> Self {
        Self { artifacts }
    }
}

impl TaskDefinition for RotateCertificateDefinition {
    const KIND: &'static str = "rotate-certificate";
    type Input = RotateCertificateInput;
    type Output = RotateCertificateOutput;

    fn capabilities(&self) -> Capabilities {
        // Safe to interrupt (a partial write is just an extra artifact
        // version) and safe to re-run later (the cert either still needs
        // rotation or it does not).
        Capabilities {
            cancellable: true,
            resumable: true,
        }
    }

    async fn run(
        &self,
        input: RotateCertificateInput,
        ctx: TaskContext,
    ) -> Result<RotateCertificateOutput, RunError> {
        let artifacts = self.artifacts.clone();
        let bind_addr = input.parse_bind_addr()?;
        tokio::select! {
            biased;
            () = ctx.cancellation_token().cancelled() => Err(RunError::Cancelled),
            rotated = pki::rotate_if_due(&artifacts, bind_addr) => {
                let rotated = rotated.map_err(|err| RunError::Failed(anyhow::Error::from(err)))?;
                Ok(RotateCertificateOutput { rotated })
            }
        }
    }
}

/// Installs the default rotation schedule if none exists yet.
///
/// Safe to call on every boot: it checks for an existing
/// `rotate-certificate` schedule and only inserts one when missing. The
/// list-then-insert is racy across processes sharing storage, but aperture
/// is a single-process gateway so this is not a concern in practice.
pub async fn install_default_rotation_schedule(
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
        interval: Interval::from_micros(ROTATION_INTERVAL_MICROS)
            .map_err(|e| anyhow::format_err!("invalid interval: {e}"))?,
        next_run_at: now,
        created_at: now,
    })
    .await?;
    Ok(())
}
