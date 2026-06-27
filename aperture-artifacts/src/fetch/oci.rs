use oci_client::manifest::{OciDescriptor, OciImageManifest};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use tokio::fs;

use super::Fetched;
use crate::blob::{BlobStore, Digest};
use crate::downloads::{Progress, ProgressWriter};
use crate::error::{ArtifactError, Result};
use crate::hash_writer::HashWriter;
use crate::media_type::MediaType;

/// Fetches OCI image layers from a registry into a blob store.
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

    /// Pulls the single layer matching `media_type` from `reference` into
    /// `store`, verifying the stored bytes against the advertised digest and
    /// reporting transferred bytes into `progress`.
    pub async fn fetch(
        &self,
        reference: &Reference,
        media_type: &MediaType,
        store: &BlobStore,
        progress: &Progress,
    ) -> Result<Fetched> {
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

        let (temp, file) = store.temp_file().await?;

        let write_result: Result<(Digest, u64)> = async {
            let mut writer = ProgressWriter::new(HashWriter::new(file), progress);
            self.client
                .pull_blob(reference, layer, &mut writer)
                .await
                .map_err(|err| ArtifactError::Fetch(err.into()))?;
            writer.into_inner().finalize().await.map_err(Into::into)
        }
        .await;

        let (digest, size) = match write_result {
            Ok(v) => v,
            Err(err) => {
                let _ = fs::remove_file(&temp).await;
                return Err(err);
            }
        };

        if digest != expected {
            let _ = fs::remove_file(&temp).await;
            return Err(ArtifactError::DigestMismatch {
                expected: expected.to_string(),
                actual: digest.to_string(),
            });
        }

        store.place(&temp, &digest).await?;
        Ok(Fetched {
            digest,
            media_type,
            size,
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
