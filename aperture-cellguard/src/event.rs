//! Domain events emitted by the cellguard driver.

use aperture_events::EventDefinition;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::snapshot::BoardIdentity;

/// Emitted when the device answered after being unreachable, and for the
/// first successful contact of a run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct DeviceConnected {
    /// Identity of the device when it is known. It is absent when the
    /// device answers polls but the identity queries failed.
    pub identity: Option<BoardIdentity>,
}

impl EventDefinition for DeviceConnected {
    const KEY: &'static str = "cellguard.device_connected";
}

/// Emitted when the device stopped answering within the staleness window,
/// or its serial port went away.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct DeviceDisconnected {
    /// Last known identity of the device, if it ever identified itself.
    pub identity: Option<BoardIdentity>,
}

impl EventDefinition for DeviceDisconnected {
    const KEY: &'static str = "cellguard.device_disconnected";
}

/// Emitted when one polled kind stopped answering while the device itself
/// is still alive. Recovery is not an event: the current state is readable
/// from the [`Cellguard`](crate::Cellguard) snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct SnapshotStale {
    /// Name of the polled kind, e.g. `"cell_voltages"`.
    pub kind: String,
    /// Last known identity of the device, if it ever identified itself.
    pub identity: Option<BoardIdentity>,
}

impl EventDefinition for SnapshotStale {
    const KEY: &'static str = "cellguard.snapshot_stale";
}
