//! Domain events emitted by the artifact store.

use aperture_events::EventDefinition;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Emitted when an artifact version is written or replaced.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ArtifactWritten {
    /// The artifact key, e.g. `"tls_server-cert"`.
    pub key: String,
    /// Content digest of the new version.
    pub digest: Option<String>,
}

impl EventDefinition for ArtifactWritten {
    const KEY: &'static str = "artifact.written";
}

/// Emitted when an artifact version is evicted.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ArtifactRemoved {
    /// The artifact key.
    pub key: String,
}

impl EventDefinition for ArtifactRemoved {
    const KEY: &'static str = "artifact.removed";
}
