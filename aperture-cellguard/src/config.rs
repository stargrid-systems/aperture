//! Driver tuning parameters.

use std::path::PathBuf;
use std::time::Duration;

/// Configuration for the cellguard driver.
///
/// `path` and `baud` come from the operator (`--cellguard`,
/// `--cellguard-baud`). The remaining fields have proven defaults and exist
/// so tests can shrink the timings.
#[derive(Debug, Clone)]
pub struct CellguardConfig {
    /// Serial device path of the bus, e.g. `/dev/ttyUSB0`.
    pub path: PathBuf,
    /// Bus baud rate. The firmware runs 115200. Higher rates distort on the
    /// optocoupler chain.
    pub baud: u32,
    /// Idle time between poll rounds.
    pub poll_interval: Duration,
    /// How long one exchange waits for a complete reply frame.
    pub reply_timeout: Duration,
    /// Poll intervals after which a kind or the device counts as stale. A
    /// kind is stale after this many consecutive failed polls. The device
    /// is disconnected once this many intervals have elapsed since its
    /// last valid reply: that threshold is elapsed time, so the
    /// open-retry backoff cadence does not shift when the disconnect
    /// fires. On a silent-but-open link the effective disconnect latency
    /// is dominated by the round duration, because each silent slot burns
    /// `2 * reply_timeout` (the wait plus the resync grace after it).
    pub stale_after: u32,
    /// Delay before the first port-open retry.
    pub open_retry_delay: Duration,
    /// Upper bound for the port-open retry backoff.
    pub open_retry_max_delay: Duration,
}

impl CellguardConfig {
    /// Builds a configuration for `path` at `baud` with the proven
    /// defaults: a 1 s poll cadence, the 2 s reply timeout the
    /// `cellguard-cli` uses, staleness after 3 missed intervals, and a 1 s
    /// to 30 s open-retry backoff.
    #[must_use]
    pub const fn new(path: PathBuf, baud: u32) -> Self {
        Self {
            path,
            baud,
            poll_interval: Duration::from_secs(1),
            reply_timeout: Duration::from_secs(2),
            stale_after: 3,
            open_retry_delay: Duration::from_secs(1),
            open_retry_max_delay: Duration::from_secs(30),
        }
    }
}
