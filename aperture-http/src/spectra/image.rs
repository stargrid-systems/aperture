//! Reading the Spectra frontend out of a squashfs image.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aperture_artifacts::Digest;
use backhand::{FilesystemReader, InnerNode, SquashfsFileReader};
use mime_guess::Mime;
use tokio::task::spawn_blocking;

use crate::conditional::Etag;

/// An opened squashfs image plus the index of files it holds.
pub(super) struct SpectraImage {
    pub(super) fs: Arc<FilesystemReader<'static>>,
    files: HashMap<String, SquashfsFileReader>,
    pub(super) etag: Etag,
}

impl SpectraImage {
    pub(super) fn open(path: &Path, digest: &Digest) -> anyhow::Result<Self> {
        let reader = BufReader::new(File::open(path)?);
        let fs = FilesystemReader::from_reader(reader)?;
        let files = fs
            .files()
            .filter_map(|node| {
                if let InnerNode::File(file) = &node.inner {
                    Some((node.fullpath.to_string_lossy().into_owned(), file.clone()))
                } else {
                    None
                }
            })
            .collect();
        let etag = Etag::from_digest(digest);
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
        if accept_br && let Some(file) = self.files.get(&format!("{base}.br")) {
            return Some(Resolved {
                file: file.clone(),
                encoding: Some("br"),
                content_type,
            });
        }
        if accept_gzip && let Some(file) = self.files.get(&format!("{base}.gz")) {
            return Some(Resolved {
                file: file.clone(),
                encoding: Some("gzip"),
                content_type,
            });
        }
        if let Some(file) = self.files.get(base) {
            return Some(Resolved {
                file: file.clone(),
                encoding: None,
                content_type,
            });
        }
        None
    }
}

/// The file picked to serve a request, with its wire encoding and type.
pub(super) struct Resolved {
    pub(super) file: SquashfsFileReader,
    pub(super) encoding: Option<&'static str>,
    pub(super) content_type: Mime,
}

/// Opens the squashfs at `path` off the async runtime.
pub(super) async fn open_image(path: PathBuf, digest: Digest) -> anyhow::Result<SpectraImage> {
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
        SpectraImage::open(Path::new(FIXTURE), &"sha256:abcd".parse().unwrap()).unwrap()
    }

    #[test]
    fn resolves_root_to_index() {
        let resolved = image().resolve("/", false, false).unwrap();
        assert_eq!(resolved.encoding, None);
        assert!(resolved.content_type.essence_str().starts_with("text/html"));
    }

    #[test]
    fn falls_back_to_spa_shell() {
        let resolved = image().resolve("/deep/link", false, false).unwrap();
        assert!(resolved.content_type.essence_str().starts_with("text/html"));
    }

    #[test]
    fn prefers_brotli_when_accepted() {
        let resolved = image().resolve("/_nuxt/app.js", true, true).unwrap();
        assert_eq!(resolved.encoding, Some("br"));
        assert!(resolved.content_type.essence_str().contains("javascript"));
    }

    #[test]
    fn serves_identity_without_accept_encoding() {
        let resolved = image().resolve("/_nuxt/app.js", false, false).unwrap();
        assert_eq!(resolved.encoding, None);
        assert!(resolved.content_type.essence_str().contains("javascript"));
    }
}
