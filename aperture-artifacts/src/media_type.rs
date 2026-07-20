//! Media type re-export.
//!
//! [`MediaType`] lives in [`aperture_storage`]. It is re-exported here so
//! existing callers can keep using `aperture_artifacts::MediaType`.

pub use aperture_storage::{InvalidMediaType, MediaType};
