//! Content digest re-export.
//!
//! [`Digest`] and friends live in [`aperture_storage`]. They are re-exported
//! here so existing callers can keep using `aperture_artifacts::Digest`.

pub use aperture_storage::{Digest, DigestAlgorithm};
