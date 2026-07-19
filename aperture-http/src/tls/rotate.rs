//! The certificate-rotation task: re-issue the leaf cert when it nears expiry.
//!
//! Driven by the periodic scheduler. The task body only writes the new cert;
//! the live TLS listener picks up the change via the artifact change feed.

use std::net::SocketAddr;
use std::time::Duration;

use aperture_artifacts::Artifacts;
use aperture_storage::{ListQuery, NewTaskSchedule, Storage};
use aperture_tasks::{Capabilities, Interval, RunError, TaskContext, TaskDefinition};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::pki;

/// Default rotation interval: 24 hours.
const ROTATION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Input for the rotate-certificate task.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateCertificateInput {
    /// The address the HTTPS listener is bound to, formatted as `host:port`.
    ///
    /// The bind IP is added to the leaf's Subject Alternative Names when it
    /// differs from the localhost defaults.
    ///
    /// Note: the value is captured at schedule-creation time. Reconfiguring
    /// the gateway to bind a different address does not retroactively update
    /// existing schedules. Delete and recreate the schedule, or restart the
    /// gateway, to pick up a new bind address.
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
        // The task body writes the new key and the new cert as two separate
        // artifact versions. If the scheduler aborted it between those writes,
        // the catalog would hold a fresh key paired with a stale cert, which
        // rustls rejects on every subsequent reload until another rotation
        // succeeds. Declaring the task non-cancellable and non-resumable keeps
        // the scheduler from interrupting it mid-write.
        Capabilities::NONE
    }

    async fn run(
        &self,
        input: RotateCertificateInput,
        _ctx: TaskContext,
    ) -> Result<RotateCertificateOutput, RunError> {
        let artifacts = self.artifacts.clone();
        let bind_addr = input.parse_bind_addr()?;
        let rotated = pki::rotate_if_due(&artifacts, bind_addr)
            .await
            .map_err(|err| RunError::Failed(anyhow::Error::from(err)))?;
        Ok(RotateCertificateOutput { rotated })
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
        interval: Interval::from_micros(ROTATION_INTERVAL.as_micros() as i64)
            .map_err(|e| anyhow::anyhow!(e).context("invalid interval"))?,
        next_run_at: now,
        created_at: now,
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::{env, fs, process};

    use aperture_storage::Storage;
    use aperture_tasks::Capabilities;

    use super::*;

    /// A temporary blob store directory removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = env::temp_dir().join(format!(
                "aperture-tls-rotate-tests-{}-{}",
                process::id(),
                uuid::Uuid::new_v4()
            ));
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    async fn fresh_store() -> Artifacts {
        let storage = Storage::open(":memory:").await.unwrap();
        let dir = TempDir::new();
        Artifacts::new(storage, dir.0.clone())
    }

    /// Locks the invariant that rotation is neither cancellable nor resumable.
    ///
    /// The capabilities flag controls whether the scheduler is allowed to
    /// interrupt the task between its two artifact writes (key then cert).
    /// Allowing interruption would land the catalog in a state rustls rejects
    /// on every subsequent reload. See
    /// `RotateCertificateDefinition::capabilities`.
    #[tokio::test]
    async fn rotation_capabilities_are_none() {
        let artifacts = fresh_store().await;
        let def = RotateCertificateDefinition::new(artifacts);
        assert_eq!(def.capabilities(), Capabilities::NONE);
    }
}
