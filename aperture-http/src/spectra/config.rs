//! Where the Spectra frontend comes from.

use std::sync::LazyLock;

use aperture_artifacts::MediaType;
use aperture_artifacts::well_known::spectra::SPECTRA;

pub(super) const SOURCE: &str = "ghcr.io/stargrid-systems/spectra:0.3.1";
pub(super) const MEDIA_TYPE: &str = "application/vnd.spectra.squashfs";

/// The image and media type the Spectra frontend is pulled from.
#[derive(Clone)]
pub struct SpectraConfig {
    /// Catalog key the frontend is stored under. Always
    /// [`aperture_artifacts::well_known::spectra::SPECTRA`]; exposed here only
    /// so callers don't need to import the constant separately.
    pub key: &'static aperture_artifacts::ArtifactKey,
    /// Image reference to pull from.
    pub source: String,
    /// Media type of the squashfs layer.
    pub media_type: MediaType,
}

impl Default for SpectraConfig {
    fn default() -> Self {
        static KEY: LazyLock<&'static aperture_artifacts::ArtifactKey> = LazyLock::new(|| &SPECTRA);
        Self {
            key: *KEY,
            source: SOURCE.to_owned(),
            media_type: MediaType::from(MEDIA_TYPE),
        }
    }
}
