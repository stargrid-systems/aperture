use aperture_tasks::ProgressHandle;
use oci_client::manifest::{OciDescriptor, OciImageManifest};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use tokio::io::AsyncWrite;

use super::{FetchMeta, Resolved};
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

    /// Resolves the layer matching `media_type` in `reference` against the
    /// registry, without transferring it. This is a manifest lookup only.
    pub async fn resolve(&self, reference: &Reference, media_type: &MediaType) -> Result<Resolved> {
        let layer = self.resolve_layer(reference, media_type).await?;
        Ok(Resolved {
            digest: layer.digest.parse()?,
            media_type: parse_registry_media_type(&layer.media_type)?,
            size: layer.size.max(0) as u64,
            version: reference.tag().map(str::to_owned),
        })
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
        let layer = self.resolve_layer(reference, media_type).await?;
        let expected: Digest = layer.digest.parse()?;
        let media_type = parse_registry_media_type(&layer.media_type)?;
        progress.set_total(layer.size.max(0) as u64);

        self.client
            .pull_blob(reference, &layer, &mut *sink)
            .await
            .map_err(|err| ArtifactError::Fetch(err.into()))?;

        Ok(FetchMeta {
            expected_digest: expected,
            media_type,
        })
    }

    /// Pulls the manifest and returns the layer descriptor matching
    /// `media_type`.
    async fn resolve_layer(
        &self,
        reference: &Reference,
        media_type: &MediaType,
    ) -> Result<OciDescriptor> {
        let (manifest, _digest) = self
            .client
            .pull_image_manifest(reference, &RegistryAuth::Anonymous)
            .await
            .map_err(|err| ArtifactError::Fetch(err.into()))?;
        find_layer(&manifest, media_type.as_str())
            .cloned()
            .ok_or_else(|| {
                ArtifactError::Fetch(anyhow::format_err!("no layer with media type {media_type}"))
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

/// Parses a media type reported by an OCI registry.
///
/// Registry manifests are trusted infrastructure, but defence in depth means
/// we still validate. A failure here bubbles up as a fetch error rather than
/// silently storing garbage.
fn parse_registry_media_type(raw: &str) -> Result<MediaType> {
    MediaType::parse(raw).ok_or_else(|| {
        ArtifactError::Fetch(anyhow::anyhow!(
            "registry returned invalid media type {raw:?}"
        ))
    })
}
