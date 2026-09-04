//! The published device state the driver keeps in memory.
//!
//! The worker rebuilds a [`DeviceSnapshot`] after every poll round and
//! publishes it through the [`Cellguard`](crate::Cellguard) handle. Nothing
//! here touches storage: persistence arrives with the entity model.

use cellguard_protocol::{
    BalancerStatus, DeviceId, RailSnapshot, SerialNumber, Snapshot, TempSnapshot,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The identity the cellcore reported for the identity request kinds.
///
/// The serial is the factory serial in lowercase hex (one byte becomes two
/// characters). `fw_version` is the raw 32-bit value from the wire. Its
/// layout (semver, monotonic counter, or something else) is defined by the
/// firmware build and not interpreted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BoardIdentity {
    /// Board model from the factory record, or
    /// [`cellguard_protocol::BOARD_MODEL_UNPROVISIONED`] when the board has
    /// none.
    pub board_model: u16,
    /// Board revision from the factory record.
    pub board_revision: u8,
    /// Raw firmware version as reported on the wire.
    pub fw_version: u32,
    /// Factory serial, lowercase hex.
    pub serial: String,
}

impl BoardIdentity {
    /// Builds the identity from the decoded `DeviceId` and `SerialNumber`
    /// payloads.
    #[must_use]
    pub fn from_protocol(id: DeviceId, serial: SerialNumber) -> Self {
        Self {
            board_model: id.board_model,
            board_revision: id.board_revision,
            fw_version: id.fw_version,
            serial: hex::encode(serial.serial),
        }
    }
}

/// One cached reading plus how fresh it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached<T> {
    /// The last successfully decoded value.
    pub data: T,
    /// When the value was read. It only advances when a poll succeeds, so
    /// the age of the data is `now - updated_at`.
    pub updated_at: Timestamp,
    /// The kind stopped answering within the staleness window. A `Nack`
    /// does not clear this flag: a rejection is an answer but brings no
    /// data.
    pub stale: bool,
}

/// Everything the driver knows about one bus node.
///
/// Today only the cellcore (bus id 1) is polled. Later changes add the
/// cellagent and cellprog as further nodes alongside it, so this type is
/// the per-node unit rather than the device snapshot itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeState {
    /// Identity reported by the node, when the identity queries ever
    /// succeeded. Kept across disconnects until a fresh query replaces it.
    pub identity: Option<BoardIdentity>,
    /// Latest cell-voltage snapshot. The `seq` field of the data increments
    /// per device-side snapshot (with `u8` wraparound), so a repeated `seq`
    /// means the device produced nothing new since the last read.
    pub cell_voltages: Option<Cached<Snapshot>>,
    /// Latest balance-current snapshot, with the same `seq` semantics as
    /// the cell voltages.
    pub balance_currents: Option<Cached<Snapshot>>,
    /// Latest supply-rail reading.
    pub rails: Option<Cached<RailSnapshot>>,
    /// Latest temperature reading.
    pub temperatures: Option<Cached<TempSnapshot>>,
    /// Latest balancing status frame.
    pub balancer_status: Option<Cached<BalancerStatus>>,
}

impl NodeState {
    /// An empty node state with no identity and no cached data.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            identity: None,
            cell_voltages: None,
            balance_currents: None,
            rails: None,
            temperatures: None,
            balancer_status: None,
        }
    }
}

/// The full state the driver publishes for the bus it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSnapshot {
    /// When this snapshot record was built. Advances on every poll round.
    pub updated_at: Timestamp,
    /// The device answered within the staleness window.
    pub connected: bool,
    /// State of the cellcore node.
    pub cellcore: NodeState,
}

impl DeviceSnapshot {
    /// A snapshot with nothing known yet: the device has not answered.
    #[must_use]
    pub const fn empty(now: Timestamp) -> Self {
        Self {
            updated_at: now,
            connected: false,
            cellcore: NodeState::empty(),
        }
    }
}
