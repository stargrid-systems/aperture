//! `CellGuard` bus driver for the Aperture gateway.
//!
//! The gateway is the bus master of the [CellGuard](https://github.com/stargrid-systems/cellguard)
//! RS485 daisy chain: it polls the cellcore node over a UART link with the
//! request kinds from `cellguard-protocol` and keeps the answers in an
//! in-memory snapshot. The driver is host-polled end to end: the bus carries
//! no unsolicited traffic.
//!
//! The driver owns three things:
//!
//! - the transport: COBS-framed packet exchange over a [`BusLink`], opened
//!   through a [`LinkFactory`] with retry backoff (the device may appear late)
//! - the poll loop: identity queries once per connection, then one round of
//!   telemetry reads per [`CellguardConfig::poll_interval`], with reply
//!   timeouts and stop-and-wait ordering like `cellguard-cli`
//! - the staleness model: a kind that stops answering for
//!   [`CellguardConfig::stale_after`] intervals is stale, a device that stops
//!   answering entirely is disconnected. Both transitions emit domain events on
//!   the [`EventBus`].
//!
//! Snapshots stay in memory. Persistence and per-MCU inventory arrive with
//! the entity model.
//!
//! # Examples
//!
//! ```
//! use aperture_cellguard::{Cellguard, CellguardConfig};
//! use aperture_events::EventBus;
//!
//! let config = CellguardConfig::new("/dev/ttyUSB0".into(), 115_200);
//! let driver = Cellguard::new(config, EventBus::new());
//! assert!(!driver.snapshot().connected);
//!
//! // Hand the poll loop to the supervisor:
//! // supervisor.spawn("cellguard", driver.into_worker());
//! ```

use std::sync::Arc;

use aperture_events::EventBus;
use arc_swap::ArcSwap;
use jiff::Timestamp;

pub use self::config::CellguardConfig;
pub use self::event::{DeviceConnected, DeviceDisconnected, SnapshotStale};
pub use self::link::{BusLink, LinkFactory, SerialLinkFactory};
pub use self::snapshot::{BoardIdentity, Cached, DeviceSnapshot, NodeState};
pub use self::worker::CellguardWorker;
use self::worker::Inner;

mod config;
mod event;
mod link;
mod snapshot;
mod transport;
mod worker;

/// Handle to the cellguard driver.
///
/// Cheap to clone: every handle shares one snapshot store. Create the
/// handle at startup, hand the worker to the supervisor, and read
/// [`Cellguard::snapshot`] from anywhere:
///
/// ```
/// use aperture_cellguard::{Cellguard, CellguardConfig};
/// use aperture_events::EventBus;
///
/// let config = CellguardConfig::new("/dev/ttyUSB0".into(), 115_200);
/// let driver = Cellguard::new(config, EventBus::new());
/// let snapshot = driver.snapshot();
/// ```
#[derive(Clone)]
pub struct Cellguard {
    inner: Arc<Inner>,
}

impl Cellguard {
    /// Creates the driver with an empty snapshot. Spawning
    /// [`Cellguard::into_worker`] starts filling it.
    #[must_use]
    pub fn new(config: CellguardConfig, event_bus: EventBus) -> Self {
        let snapshots = ArcSwap::from_pointee(DeviceSnapshot::empty(Timestamp::now()));
        Self {
            inner: Arc::new(Inner {
                config,
                event_bus,
                snapshots,
            }),
        }
    }

    /// The latest device state. The worker republishes it after every poll
    /// round, so readers never observe a partially updated snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<DeviceSnapshot> {
        self.inner.snapshots.load_full()
    }

    /// Builds the poll worker for the configured serial port. Spawn it
    /// under the name `"cellguard"`.
    #[must_use]
    pub fn into_worker(self) -> CellguardWorker<SerialLinkFactory> {
        let factory =
            SerialLinkFactory::new(self.inner.config.path.clone(), self.inner.config.baud);
        CellguardWorker::new(self.inner, factory)
    }
}
