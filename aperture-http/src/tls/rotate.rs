//! Certificate rotation task: re-issues the leaf cert when it nears expiry.

use aperture_artifacts::Artifacts;
use aperture_storage::{ListQuery, NewTaskSchedule, Storage};
use aperture_tasks::{Capabilities, Interval, RunError, TaskContext, TaskDefinition};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
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
}

impl RotateCertificateDefinition {
    pub const fn new(artifacts: Artifacts) -> Self {
        Self { artifacts }
    }
}

impl TaskDefinition for RotateCertificateDefinition {
    const KEY: &'static str = "rotate-certificate";
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
        Artifacts::new(storage, dir.0.clone())
    }

    #[tokio::test]
    async fn rotation_capabilities_are_none() {
        let artifacts = fresh_store().await;
        let def = RotateCertificateDefinition::new(artifacts);
        assert_eq!(def.capabilities(), Capabilities::NONE);
    }
}
