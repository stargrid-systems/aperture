//! The download task: fetching an artifact, modelled as a [`TaskDefinition`].

use aperture_storage::ArtifactKey;
use aperture_tasks::{Capabilities, RunError, TaskContext, TaskDefinition};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::artifacts::{Artifacts, FetchRequest, FetchSource};
use crate::media_type::MediaType;

/// Input for a download task: what to fetch and where from.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DownloadInput {
    /// Logical key to record the artifact under.
    pub key: ArtifactKey,
    /// Where and how to fetch it from.
    pub source: DownloadSource,
}

/// Where a download fetches from.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownloadSource {
    /// A layer of an OCI image.
    Oci {
        /// The image reference, for example `ghcr.io/org/image:tag`.
        reference: String,
        /// The media type of the layer to pull.
        media_type: MediaType,
    },
}

impl From<DownloadSource> for FetchSource {
    fn from(source: DownloadSource) -> Self {
        match source {
            DownloadSource::Oci {
                reference,
                media_type,
            } => FetchSource::Oci {
                reference,
                media_type,
            },
        }
    }
}

/// Output of a download task: the stored version that resulted.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DownloadOutput {
    /// Content digest of the stored blob.
    pub digest: String,
    /// Stored blob size in bytes.
    pub size_bytes: u64,
    /// Human-readable version, if known.
    pub version: Option<String>,
}

/// The download task kind. Fetches an artifact into the blob store and records
/// the version, reporting transferred bytes as progress.
pub struct DownloadDefinition {
    artifacts: Artifacts,
}

impl DownloadDefinition {
    /// Creates the definition over `artifacts`.
    pub fn new(artifacts: Artifacts) -> Self {
        Self { artifacts }
    }
}

impl TaskDefinition for DownloadDefinition {
    const KIND: &'static str = "download";
    type Input = DownloadInput;
    type Output = DownloadOutput;

    fn capabilities(&self) -> Capabilities {
        // A download is safe to stop and re-run, and the blob store discards a
        // partial fetch, so it is both cancellable and resumable.
        Capabilities {
            cancellable: true,
            resumable: true,
        }
    }

    async fn run(
        &self,
        input: DownloadInput,
        ctx: TaskContext,
    ) -> Result<DownloadOutput, RunError> {
        let request = FetchRequest {
            key: input.key,
            source: input.source.into(),
        };
        // Stop the fetch as soon as cancellation is requested. The partial blob
        // is staged in a temp file and never placed, so a later sync reclaims it.
        let artifact = tokio::select! {
            biased;
            () = ctx.cancellation_token().cancelled() => return Err(RunError::Cancelled),
            result = self.artifacts.download(request, ctx.progress()) => {
                result.map_err(|err| RunError::Failed(err.into()))?
            }
        };
        Ok(DownloadOutput {
            digest: artifact.digest.to_string(),
            size_bytes: artifact.size_bytes,
            version: artifact.version,
        })
    }
}
