//! The certificate-rotation task: re-issue the leaf cert when it nears expiry.
//!
//! Driven by the periodic scheduler (see `aperture::serve`). The task body
//! only writes the new cert; the live TLS listener picks up the change via the
//! artifact change feed (see `tls_reload_watcher` in `aperture::serve`).

use std::net::SocketAddr;
use std::sync::Arc;

use aperture_artifacts::Artifacts;
use aperture_tasks::{Capabilities, RunError, TaskContext, TaskDefinition};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::pki;

/// Input for the rotate-certificate task.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateCertificateInput {
    /// The address the HTTPS listener is bound to, formatted as `host:port`.
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
    /// Whether the leaf was actually re-issued. `false` means the existing
    /// cert still had enough remaining validity and no rotation was needed.
    pub rotated: bool,
}

/// Task definition for periodic certificate rotation.
pub struct RotateCertificateDefinition {
    artifacts: Arc<Artifacts>,
}

impl RotateCertificateDefinition {
    /// Creates the definition over `artifacts`.
    pub fn new(artifacts: Arc<Artifacts>) -> Self {
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
        let artifacts = Arc::clone(&self.artifacts);
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
