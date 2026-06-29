//! Tracing initialization for the Aperture gateway.
//!
//! Sets up two layers on the global subscriber:
//!
//! - A `fmt` layer that writes to stdout, filtered by `EnvFilter`. The default
//!   filter shows aperture crates at `INFO` and everything else at `WARN`.
//!   Override with `RUST_LOG`.
//! - A [`DbLogLayer`] that persists spans and events to the database.
//!
//! [`DbLogLayer`]: layer::DbLogLayer

use aperture_artifacts::LogWriter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use self::layer::{DbLogLayer, WorkerHandle};

mod layer;

/// Default console filter: aperture crates at INFO, everything else at WARN.
const DEFAULT_FILTER: &str = "aperture=info,aperture_storage=info,aperture_http=info,aperture_artifacts=info,warn";

/// Sets up the tracing subscriber with a fmt layer (stdout) and a database
/// layer.
///
/// Returns a [`WorkerHandle`] for clean shutdown. Keep it alive for the
/// lifetime of the application and call [`WorkerHandle::shutdown`] before
/// exiting to flush pending records.
pub fn init(writer: LogWriter) -> WorkerHandle {
    use tracing_subscriber::EnvFilter;

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let fmt_layer = fmt::layer().with_filter(filter);
    let (db_layer, handle) = DbLogLayer::spawn(writer);
    let db_layer = db_layer.with_filter(LevelFilter::TRACE);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(db_layer)
        .init();

    handle
}