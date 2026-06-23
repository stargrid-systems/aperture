use aperture_core::VersionInfo;
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
