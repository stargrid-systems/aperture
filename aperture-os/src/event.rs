//! Domain events emitted by OS integration.

use aperture_events::EventDefinition;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Emitted after the system hostname was successfully applied via
/// systemd-hostnamed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct HostnameApplied {
    /// The hostname that was applied.
    pub hostname: String,
}

impl EventDefinition for HostnameApplied {
    const KEY: &'static str = "os.hostname_applied";
}
