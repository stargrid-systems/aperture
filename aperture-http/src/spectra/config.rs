//! Where the Spectra frontend comes from.

use aperture_artifacts::{ArtifactKey, MediaType};

const SOURCE: &str = "ghcr.io/stargrid-systems/spectra:0.3.1";
const MEDIA_TYPE: &str = "application/vnd.spectra.squashfs";

const SPECTRA_KEY: ArtifactKey = ArtifactKey::from_static("spectra");

/// The image and media type the Spectra frontend is pulled from.
#[derive(Clone)]
pub struct SpectraConfig {
    /// Catalog key the frontend is stored under.
    pub key: ArtifactKey,
    /// Image reference to pull from.
    pub source: String,
    /// Media type of the squashfs layer.
    pub media_type: MediaType,
}

impl Default for SpectraConfig {
    fn default() -> Self {
        Self {
            key: SPECTRA_KEY.clone(),
            source: SOURCE.to_owned(),
            media_type: MEDIA_TYPE.parse().expect("well-known media type"),
        }
    }
}
