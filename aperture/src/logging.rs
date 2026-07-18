//! Tracing initialization for the Aperture gateway.
//!
//! Sets up two layers on the global subscriber:
//!
//! - A `fmt` layer that writes to stdout, filtered by `EnvFilter`. The default
//!   filter shows aperture crates at `INFO` and everything else at `WARN`.
//!   Override with `RUST_LOG`.
//! - A [`DbLogLayer`] that persists spans and events to the database.
//!
//! The `log` crate is bridged into tracing by `tracing-subscriber`'s
//! `tracing-log` feature. `SubscriberInitExt::init` calls `LogTracer::init`
//! when that feature is active. The real target, file, and line from `log`
//! records are carried as `log.*` fields and extracted by [`DbLogLayer`]
//! rather than using the static `"log"` target.
//!
//! [`DbLogLayer`]: layer::DbLogLayer

use aperture_storage::LogRepository;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};
use uuid::Uuid;

use self::layer::{DbLogLayer, LogWorker};

mod layer;

/// Default console filter: aperture crates at INFO, everything else at WARN.
const DEFAULT_FILTER: &str = "aperture=info,warn";

/// Constructs the database log layer and its background worker. Hand the
/// layer to [`init`] and hand the worker to a [`Supervisor`].
///
/// [`Supervisor`]: crate::runtime::Supervisor
pub fn build(repo: LogRepository, boot_id: Uuid) -> (DbLogLayer, LogWorker) {
    DbLogLayer::new(repo, boot_id)
}

/// Installs `db_layer` (along with a stdout fmt layer) on the global
/// subscriber. Must be called at most once per process.
pub fn init(db_layer: DbLogLayer) {
    let console_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let fmt_layer = fmt::layer().with_filter(console_filter);

    let db_filter = Targets::new()
        .with_default(LevelFilter::TRACE)
        .with_target("backhand", LevelFilter::DEBUG)
        .with_target("turso", LevelFilter::INFO);

    let db_layer = db_layer.with_filter(db_filter);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(db_layer)
        .init();
}
