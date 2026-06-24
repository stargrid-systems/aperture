use aperture_artifacts::DownloadProgress;
use aperture_core::VersionInfo;
use jiff::Timestamp;
use serde::Serialize;
use utoipa::ToSchema;

/// Version information returned by `GET /api/v1/version`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VersionResponse {
    /// Version of the Aperture gateway.
    pub aperture: String,
}

impl From<VersionInfo> for VersionResponse {
    fn from(info: VersionInfo) -> Self {
        Self {
            aperture: info.aperture.to_owned(),
        }
    }
}

/// One download currently in flight, returned by
/// `GET /api/v1/artifacts/downloads`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DownloadResponse {
    /// Logical artifact name.
    pub name: String,
    /// Where it is being fetched from.
    pub source: String,
    /// When the download started, as an ISO 8601 timestamp.
    pub started_at: Timestamp,
    /// Bytes transferred so far.
    pub done_bytes: u64,
    /// Expected total bytes, if known.
    pub total_bytes: Option<u64>,
}

impl From<DownloadProgress> for DownloadResponse {
    fn from(progress: DownloadProgress) -> Self {
        Self {
            name: progress.name,
            source: progress.source,
            started_at: progress.started_at,
            done_bytes: progress.done_bytes,
            total_bytes: progress.total_bytes,
        }
    }
}
