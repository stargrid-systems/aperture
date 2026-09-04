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

use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

use self::layer::{DbLogLayer, DeferredLogWorker};

mod layer;

/// Default console filter: aperture crates at INFO, everything else at WARN.
const DEFAULT_FILTER: &str = "aperture=info,warn";

/// Installs the tracing subscriber.
///
/// The DB layer buffers records to a channel until
/// [`DeferredLogWorker::connect`] produces a runnable worker. Call this
/// as early as possible so startup is captured.
pub fn init() -> DeferredLogWorker {
    let (db_layer, deferred) = DbLogLayer::new();

    let console_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let fmt_layer = fmt::layer().with_filter(console_filter);

    let db_filter = Targets::new()
        .with_default(LevelFilter::TRACE)
        // backhand doesn't really output anything useful at TRACE level.
        .with_target("backhand", LevelFilter::DEBUG)
        // HACK: At DEBUG level we get a feedback loop :(
        .with_target("turso", LevelFilter::INFO);
    let db_layer = db_layer.with_filter(db_filter);

    // Only one global subscriber can exist. The serve tests install their
    // own, so a second init must degrade to a no-op instead of panicking.
    if let Err(err) = tracing_subscriber::registry()
        .with(fmt_layer)
        .with(db_layer)
        .try_init()
    {
        // Nothing is installed, so tracing cannot report this. In
        // production serve runs once per process; the realistic trigger
        // is tests, whose output captures stderr.
        eprintln!("failed to install tracing subscriber: {err}");
    }

    deferred
}
