//! Serving the Spectra frontend straight from a squashfs image.
//!
//! The image is one content-addressed blob. We read files out of it on demand
//! with a userspace squashfs reader, so there is no unpacked copy on disk. The
//! image is held behind a swappable slot so the frontend can upgrade at
//! runtime. Each response carries an `ETag` from the image digest.
//!
//! The frontend is fetched lazily. The first request for a missing frontend
//! kicks off a background download and gets a small placeholder page that
//! refreshes itself until the real interface is ready.

pub use self::config::SpectraConfig;
pub use self::manager::{Spectra, SpectraWorker};
pub use self::serve::fallback;

mod config;
mod image;
mod manager;
mod serve;
