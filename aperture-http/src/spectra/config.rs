//! Where the Spectra frontend comes from.

use std::sync::LazyLock;

use aperture_artifacts::{ArtifactKey, MediaType};

pub(super) const SOURCE: &str = "ghcr.io/stargrid-systems/spectra:0.3.1";
pub(super) const MEDIA_TYPE: &str = "application/vnd.spectra.squashfs";

static SPECTRA_KEY: LazyLock<ArtifactKey> =
    LazyLock::new(|| ArtifactKey::new("spectra").expect("well-known key"));

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
            media_type: MediaType::from(MEDIA_TYPE),
        }
    }
}
