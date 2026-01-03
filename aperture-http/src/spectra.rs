//! Directory layout:

use std::path::PathBuf;

use miette::IntoDiagnostic;
use oci_client::Reference;
use tokio::fs;
use tower_http::services::{ServeDir, ServeFile};

mod oci;

const LIVE_DIR: &str = "live";
const DOWNLOAD_DIR: &str = "download";

pub struct Spectra {
    path: PathBuf,
}

impl Spectra {
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub async fn prep(&mut self) -> miette::Result<()> {
        let image = Reference::with_tag(
            self::oci::DEFAULT_REGISTRY.to_owned(),
            self::oci::DEFAULT_REPOSITORY.to_owned(),
            self::oci::DEFAULT_TAG.to_owned(),
        );
        self.pull(&image).await?;
        Ok(())
    }

    pub async fn pull(&mut self, image: &Reference) -> miette::Result<()> {
        tracing::info!(
            registry = image.registry(),
            repository = image.repository(),
            tag = image.tag(),
            "pulling spectra image"
        );
        let download_dir = self.path.join(DOWNLOAD_DIR);
        let _ = fs::remove_dir_all(&download_dir).await;
        fs::create_dir_all(&download_dir).await.into_diagnostic()?;
        self::oci::download_to(&download_dir, &image).await?;

        tracing::info!("download complete, updating live directory");
        let live_dir = self.path.join(LIVE_DIR);
        let _ = fs::remove_dir_all(&live_dir).await;
        fs::rename(download_dir, live_dir).await.into_diagnostic()?;
        tracing::info!("spectra update complete");
        Ok(())
    }

    pub fn service(&self) -> ServeDir<ServeFile> {
        let live_dir = self.path.join(LIVE_DIR);
        let fallback = ServeFile::new(live_dir.join("200.html"));
        ServeDir::new(live_dir)
            .append_index_html_on_directories(true)
            .precompressed_br()
            .precompressed_gzip()
            .fallback(fallback)
    }
}
