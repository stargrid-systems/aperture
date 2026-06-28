use oci_client::manifest::{OciDescriptor, OciImageManifest};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use tokio::io::AsyncWrite;

use aperture_tasks::ProgressHandle;

use super::FetchMeta;
use crate::digest::Digest;
use crate::error::{ArtifactError, Result};
use crate::media_type::MediaType;

/// Fetches OCI image layers from a registry.
pub struct OciFetcher {
    client: Client,
}

impl OciFetcher {
    /// Creates a fetcher with the default anonymous registry client.
    pub fn new() -> Self {
        Self {
            client: Client::default(),
        }
    }

    /// Streams the single layer matching `media_type` from `reference` into
    /// `sink`, reporting transferred bytes into `progress`. Returns the digest
    /// the registry advertises so the caller can verify the stored bytes.
    pub async fn fetch(
        &self,
        reference: &Reference,
        media_type: &MediaType,
        sink: &mut (dyn AsyncWrite + Unpin + Send),
        progress: &ProgressHandle,
    ) -> Result<FetchMeta> {
        let (manifest, _digest) = self
            .client
            .pull_image_manifest(reference, &RegistryAuth::Anonymous)
            .await
            .map_err(|err| ArtifactError::Fetch(err.into()))?;
        let layer = find_layer(&manifest, media_type.as_str()).ok_or_else(|| {
            ArtifactError::Fetch(anyhow::format_err!("no layer with media type {media_type}"))
        })?;
        let expected: Digest = layer.digest.parse()?;
        let media_type = MediaType::from(layer.media_type.as_str());
        progress.set_total(layer.size.max(0) as u64);

        self.client
            .pull_blob(reference, layer, &mut *sink)
            .await
            .map_err(|err| ArtifactError::Fetch(err.into()))?;

        Ok(FetchMeta {
            expected_digest: expected,
            media_type,
        })
    }
}

impl Default for OciFetcher {
    fn default() -> Self {
        Self::new()
    }
}

fn find_layer<'a>(manifest: &'a OciImageManifest, media_type: &str) -> Option<&'a OciDescriptor> {
    manifest
        .layers
        .iter()
        .find(|layer| layer.media_type == media_type)
}
