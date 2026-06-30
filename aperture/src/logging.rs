//! Tracing initialization for the Aperture gateway.
//!
//! Sets up two layers on the global subscriber:
//!
//! - A `fmt` layer that writes to stdout, filtered by `EnvFilter`. The default
//!   filter shows aperture crates at `INFO` and everything else at `WARN`.
//!   Override with `RUST_LOG`.
//! - A [`DbLogLayer`] that persists spans and events to the database.
//!
//! The `log` crate is bridged into tracing via [`LogTracer`] so that records
//! from dependencies (turso, tantivy, backhand, etc.) appear as tracing events.
//! The real target, file, and line are carried as `log.*` fields and extracted
//! by [`DbLogLayer`] rather than using the static `"log"` target.
//!
//! [`DbLogLayer`]: layer::DbLogLayer
//!
//! [`LogTracer`]: tracing_log::LogTracer

use aperture_artifacts::LogWriter;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use self::layer::{DbLogLayer, WorkerHandle};

mod layer;

/// Default console filter: aperture crates at INFO, everything else at WARN.
const DEFAULT_FILTER: &str =
    "aperture=info,aperture_storage=info,aperture_http=info,aperture_artifacts=info,warn";

/// Sets up the tracing subscriber with a fmt layer (stdout) and a database
/// layer.
///
/// Returns a [`WorkerHandle`] for clean shutdown. Keep it alive for the
/// lifetime of the application and call [`WorkerHandle::shutdown`] before
/// exiting to flush pending records.
pub fn init(writer: LogWriter, boot_id: String) -> WorkerHandle {
    use tracing_subscriber::EnvFilter;

    // Bridge `log` crate records into tracing events. This only fails if a
    // global logger was already installed, which does not happen in our
    // binary.
    let _ = tracing_log::LogTracer::init();

    let console_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let fmt_layer = fmt::layer().with_filter(console_filter);

    // The DB layer captures everything at TRACE except crates whose
    // diagnostics are caused by the database engine itself. Without this,
    // turso emits trace events for each insert, the layer captures them,
    // and a feedback loop fills the channel.
    //
    // Log-bridged events carry metadata target "log", so the Targets filter
    // cannot exclude them by their real target. The DbLogLayer drops those
    // in `on_event` after extracting the real target from the `log.target`
    // field.
    let db_filter = Targets::new()
        .with_target("turso", LevelFilter::WARN)
        .with_target("tantivy", LevelFilter::WARN)
        .with_target("backhand", LevelFilter::WARN)
        .with_default(LevelFilter::TRACE);

    let (db_layer, handle) = DbLogLayer::spawn(writer, boot_id);
    let db_layer = db_layer.with_filter(db_filter);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(db_layer)
        .init();

    handle
}