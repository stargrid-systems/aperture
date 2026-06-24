//! Reading the Spectra frontend out of a squashfs image.

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::HeaderValue;
use backhand::{FilesystemReader, InnerNode};
use mime_guess::Mime;
use tokio::task::spawn_blocking;

/// An opened squashfs image plus the index of files it holds.
pub(super) struct SpectraImage {
    pub(super) fs: Arc<FilesystemReader<'static>>,
    files: HashSet<String>,
    pub(super) etag: HeaderValue,
}

impl SpectraImage {
    pub(super) fn open(path: &Path, digest: &str) -> anyhow::Result<Self> {
        let reader = BufReader::new(File::open(path)?);
        let fs = FilesystemReader::from_reader(reader)?;
        let files = fs
            .files()
            .filter(|node| matches!(node.inner, InnerNode::File(_)))
            .map(|node| node.fullpath.to_string_lossy().into_owned())
            .collect();
        let etag = HeaderValue::from_str(&format!("\"{digest}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("\"spectra\""));
        Ok(Self {
            fs: Arc::new(fs),
            files,
            etag,
        })
    }

    fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Picks the stored file to serve for `request_path`, honouring the
    /// accepted encodings and falling back to the SPA shell.
    pub(super) fn resolve(
        &self,
        request_path: &str,
        accept_br: bool,
        accept_gzip: bool,
    ) -> Option<Resolved> {
        let primary = if request_path.is_empty() || request_path == "/" {
            "/index.html".to_owned()
        } else {
            request_path.to_owned()
        };
        self.pick(&primary, accept_br, accept_gzip)
            .or_else(|| self.pick("/200.html", accept_br, accept_gzip))
    }

    fn pick(&self, base: &str, accept_br: bool, accept_gzip: bool) -> Option<Resolved> {
        let content_type = mime_guess::from_path(base).first_or_octet_stream();
        if accept_br && self.files.contains(&format!("{base}.br")) {
            return Some(Resolved {
                stored: format!("{base}.br"),
                encoding: Some("br"),
                content_type,
            });
        }
        if accept_gzip && self.files.contains(&format!("{base}.gz")) {
            return Some(Resolved {
                stored: format!("{base}.gz"),
                encoding: Some("gzip"),
                content_type,
            });
        }
        if self.files.contains(base) {
            return Some(Resolved {
                stored: base.to_owned(),
                encoding: None,
                content_type,
            });
        }
        None
    }
}

/// The file picked to serve a request, with its wire encoding and type.
pub(super) struct Resolved {
    pub(super) stored: String,
    pub(super) encoding: Option<&'static str>,
    pub(super) content_type: Mime,
}

/// Opens the squashfs at `path` off the async runtime.
pub(super) async fn open_image(path: PathBuf, digest: String) -> anyhow::Result<SpectraImage> {
    let image = spawn_blocking(move || SpectraImage::open(&path, &digest)).await??;
    if image.is_empty() {
        tracing::warn!("spectra image has no servable files; check how it was packed");
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/site.sqfs");

    fn image() -> SpectraImage {
        SpectraImage::open(Path::new(FIXTURE), "sha256:test").unwrap()
    }

    #[test]
    fn resolves_root_to_index() {
        let resolved = image().resolve("/", false, false).unwrap();
        assert_eq!(resolved.stored, "/index.html");
        assert_eq!(resolved.encoding, None);
        assert!(resolved.content_type.essence_str().starts_with("text/html"));
    }

    #[test]
    fn falls_back_to_spa_shell() {
        let resolved = image().resolve("/deep/link", false, false).unwrap();
        assert_eq!(resolved.stored, "/200.html");
    }

    #[test]
    fn prefers_brotli_when_accepted() {
        let resolved = image().resolve("/_nuxt/app.js", true, true).unwrap();
        assert_eq!(resolved.stored, "/_nuxt/app.js.br");
        assert_eq!(resolved.encoding, Some("br"));
        assert!(resolved.content_type.essence_str().contains("javascript"));
    }

    #[test]
    fn serves_identity_without_accept_encoding() {
        let resolved = image().resolve("/_nuxt/app.js", false, false).unwrap();
        assert_eq!(resolved.stored, "/_nuxt/app.js");
        assert_eq!(resolved.encoding, None);
    }
}
