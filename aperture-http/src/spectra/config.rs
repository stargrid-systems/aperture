//! Where the Spectra frontend comes from.

use aperture_artifacts::MediaType;

pub(super) const NAME: &str = "spectra";
pub(super) const SOURCE: &str = "ghcr.io/stargrid-systems/spectra:0.3.0";
pub(super) const MEDIA_TYPE: &str = "application/vnd.spectra.squashfs";

/// The image and media type the Spectra frontend is pulled from.
#[derive(Clone)]
pub struct SpectraConfig {
    /// Catalog name.
    pub name: String,
    /// Image reference to pull from.
    pub source: String,
    /// Media type of the squashfs layer.
    pub media_type: MediaType,
}

impl Default for SpectraConfig {
    fn default() -> Self {
        Self {
            name: NAME.to_owned(),
            source: SOURCE.to_owned(),
            media_type: MediaType::from(MEDIA_TYPE),
        }
    }
}
