//! Certificate tasks: periodic rotation and identity regeneration.

use std::net::SocketAddr;
use std::sync::Arc;

use aperture_artifacts::Artifacts;
use aperture_storage::{ListQuery, NewTaskSchedule, Storage};
use aperture_tasks::{Capabilities, Interval, RunError, TaskContext, TaskDefinition, keys};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use utoipa::ToSchema;

use super::pki;

const ROTATION_INTERVAL: SignedDuration = SignedDuration::from_secs(24 * 60 * 60);

/// Rotation takes no parameters (identity-preserving).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct RotateCertificateInput {}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateCertificateOutput {
    /// Whether the leaf was actually re-issued.
    pub rotated: bool,
}

/// Task definition for periodic certificate rotation.
pub struct RotateCertificateDefinition {
    artifacts: Artifacts,
    cert_lock: Arc<Mutex<()>>,
}

impl RotateCertificateDefinition {
    pub const fn new(artifacts: Artifacts, cert_lock: Arc<Mutex<()>>) -> Self {
        Self {
            artifacts,
            cert_lock,
        }
    }
}

impl TaskDefinition for RotateCertificateDefinition {
    const KEY: &'static str = keys::ROTATE_CERTIFICATE;

    type Input = RotateCertificateInput;
    type Output = RotateCertificateOutput;

    fn capabilities(&self) -> Capabilities {
        // Non-cancellable: the task writes key then cert as separate versions.
        // Interruption between writes would leave a mismatch rustls rejects.
        Capabilities::NONE
    }

    async fn run(
        &self,
        _input: RotateCertificateInput,
        _ctx: TaskContext,
    ) -> Result<RotateCertificateOutput, RunError> {
        let _guard = self.cert_lock.lock().await;
        let artifacts = self.artifacts.clone();
        let rotated = pki::rotate_if_due(&artifacts)
            .await
            .map_err(|err| RunError::Failed(anyhow::Error::from(err)))?;
        Ok(RotateCertificateOutput { rotated })
    }
}

/// Installs the default rotation schedule if none exists yet.
///
/// # Errors
///
/// Returns an error if the storage layer fails to list or create schedules.
pub async fn install_default_rotation_schedule(storage: &Storage) -> anyhow::Result<()> {
    let repo = storage.task_schedules()?;
    let existing = repo.list(&ListQuery::default()).await?;
    let already = existing
        .items
        .iter()
        .any(|s| s.key == RotateCertificateDefinition::KEY);
    if already {
        return Ok(());
    }
    let now = Timestamp::now();
    repo.create(&NewTaskSchedule {
        key: RotateCertificateDefinition::KEY.to_owned(),
        input: serde_json::json!({}),
        interval: Interval::new(ROTATION_INTERVAL)?,
        next_run_at: now,
        created_at: now,
    })
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegenerateCertificateInput {
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct RegenerateCertificateOutput {}

pub struct RegenerateCertificateDefinition {
    artifacts: Artifacts,
    bind_addr: SocketAddr,
    cert_lock: Arc<Mutex<()>>,
}

impl RegenerateCertificateDefinition {
    pub const fn new(
        artifacts: Artifacts,
        bind_addr: SocketAddr,
        cert_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            artifacts,
            bind_addr,
            cert_lock,
        }
    }
}

impl TaskDefinition for RegenerateCertificateDefinition {
    const KEY: &'static str = keys::REGENERATE_CERTIFICATE;

    type Input = RegenerateCertificateInput;
    type Output = RegenerateCertificateOutput;

    fn capabilities(&self) -> Capabilities {
        Capabilities::NONE
    }

    async fn run(
        &self,
        input: RegenerateCertificateInput,
        _ctx: TaskContext,
    ) -> Result<RegenerateCertificateOutput, RunError> {
        let _guard = self.cert_lock.lock().await;
        pki::regenerate_leaf_for_identity(
            &self.artifacts,
            self.bind_addr,
            input.hostname.as_deref(),
        )
        .await
        .map_err(|err| RunError::Failed(anyhow::Error::from(err)))?;
        Ok(RegenerateCertificateOutput {})
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::{env, fs, process};

    use aperture_storage::Storage;
    use aperture_tasks::Capabilities;

    use super::*;

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
        let event_bus = aperture_events::EventBus::new();
        Artifacts::new(storage, dir.0.clone(), event_bus)
    }

    #[tokio::test]
    async fn rotation_capabilities_are_none() {
        let artifacts = fresh_store().await;
        let lock = Arc::new(Mutex::new(()));
        let def = RotateCertificateDefinition::new(artifacts, lock);
        assert_eq!(def.capabilities(), Capabilities::NONE);
    }
}
