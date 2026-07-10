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

use self::layer::{DbLogLayer, WorkerHandle};

mod layer;

/// Default console filter: aperture crates at INFO, everything else at WARN.
const DEFAULT_FILTER: &str = "aperture=info,warn";

/// Sets up the tracing subscriber with a fmt layer (stdout) and a database
/// layer.
///
/// Returns a [`WorkerHandle`] for clean shutdown. Keep it alive for the
/// lifetime of the application and call [`WorkerHandle::shutdown`] before
/// exiting to flush pending records.
pub fn init(repo: LogRepository, boot_id: Uuid) -> WorkerHandle {
    let console_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let fmt_layer = fmt::layer().with_filter(console_filter);

    let db_filter = Targets::new()
        .with_default(LevelFilter::TRACE)
        // backhand doesn't really output anything useful at TRACE level.
        .with_target("backhand", LevelFilter::DEBUG)
        // HACK: At DEBUG level we get a feedback loop :(
        .with_target("turso", LevelFilter::INFO);

    let (db_layer, handle) = DbLogLayer::spawn(repo, boot_id);
    let db_layer = db_layer.with_filter(db_filter);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(db_layer)
        .init();

    handle
}
