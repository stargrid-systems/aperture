use std::path::Path;

use async_compression::tokio::bufread::GzipDecoder;
use miette::{Context, IntoDiagnostic};
use oci_client::manifest::{OciDescriptor, OciImageManifest};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use tokio::io::{self, BufReader};
use tokio::try_join;
use tokio_tar::Archive;

const MEDIA_TYPE: &str = "application/vnd.spectra.tar+gzip";

pub const DEFAULT_REGISTRY: &str = "ghcr.io";
pub const DEFAULT_REPOSITORY: &str = "stargrid-systems/spectra";
pub const DEFAULT_TAG: &str = "0.2.0";

const MAX_BUFFER_SIZE: usize = 8 * 1024 * 1024; // 8 MiB

/// Downloads the spectra site to the given directory.
///
/// The directory should be empty.
pub async fn download_to(path: &Path, image: &Reference) -> miette::Result<()> {
    let client = Client::default();
    let (manifest, _digest) = client
        .pull_image_manifest(&image, &RegistryAuth::Anonymous)
        .await
        .into_diagnostic()?;
    let layer_desc = find_layer(&manifest).wrap_err("no spectra layer found in image")?;

    let (read, write) = io::simplex(MAX_BUFFER_SIZE);
    let mut archive = Archive::new(GzipDecoder::new(BufReader::new(read)));

    let pull_fut = async {
        client
            .pull_blob(&image, &layer_desc, write)
            .await
            .into_diagnostic()
    };
    let unpack_fut = async { archive.unpack(path).await.into_diagnostic() };

    try_join!(pull_fut, unpack_fut)?;
    Ok(())
}

fn find_layer(manifest: &OciImageManifest) -> Option<&OciDescriptor> {
    manifest
        .layers
        .iter()
        .find(|layer| layer.media_type == MEDIA_TYPE)
}
