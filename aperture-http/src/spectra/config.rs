//! Where the Spectra frontend comes from.

use aperture_artifacts::well_known::spectra::SPECTRA;
use aperture_artifacts::{ArtifactKey, MediaType};

pub(super) const SOURCE: &str = "ghcr.io/stargrid-systems/spectra:0.3.1";
pub(super) const MEDIA_TYPE: &str = "application/vnd.spectra.squashfs";

/// The image and media type the Spectra frontend is pulled from.
#[derive(Clone)]
pub struct SpectraConfig {
    /// Catalog key the frontend is stored under. Always
    /// [`aperture_artifacts::well_known::spectra::SPECTRA`]; exposed here only
    /// so callers don't need to import the constant separately.
    pub key: ArtifactKey,
    /// Image reference to pull from.
    pub source: String,
    /// Media type of the squashfs layer.
    pub media_type: MediaType,
}

impl Default for SpectraConfig {
    fn default() -> Self {
        Self {
            key: SPECTRA.clone(),
            source: SOURCE.to_owned(),
            media_type: MediaType::from(MEDIA_TYPE),
        }
    }
}
