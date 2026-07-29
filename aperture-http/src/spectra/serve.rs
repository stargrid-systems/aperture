//! Serving the current frontend over HTTP, or a placeholder while it installs.

use std::io::{self, Read};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG, RETRY_AFTER, VARY,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use backhand::{FilesystemReader, FilesystemReaderFile, SquashfsFileReader};
use bytes::BytesMut;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, spawn_blocking};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;

use super::image::SpectraImage;
use crate::AppState;

const PLACEHOLDER: &str = include_str!("installing.html");

const CACHE_NO_CACHE: HeaderValue = HeaderValue::from_static("no-cache");
const CACHE_NO_STORE: HeaderValue = HeaderValue::from_static("no-store");
const VARY_ACCEPT_ENCODING: HeaderValue = HeaderValue::from_static("accept-encoding");
const NOSNIFF: HeaderValue = HeaderValue::from_static("nosniff");
const OCTET_STREAM: HeaderValue = HeaderValue::from_static("application/octet-stream");

/// Serves the current frontend, or a self-refreshing placeholder while it is
/// still being installed.
pub(crate) async fn fallback(State(state): State<AppState>, request: Request) -> Response {
    let spectra = state.spectra();
    match spectra.current() {
        Some(image) => serve(image, request),
        None => {
            spectra.ensure_started();
            placeholder()
        }
    }
}

fn serve(image: Arc<SpectraImage>, request: Request) -> Response {
    if image.etag.matches_if_none_match(request.headers()) {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (ETAG, image.etag.clone().into()),
                (CACHE_CONTROL, CACHE_NO_CACHE.clone()),
                // The 200 response varies by Accept-Encoding, so the 304 must
                // advertise the same Vary per RFC 9110 section 15.4.5. The
                // ETag alone does not distinguish encodings.
                (VARY, VARY_ACCEPT_ENCODING.clone()),
            ],
        )
            .into_response();
    }

    let accepted = AcceptedEncodings::from_headers(request.headers());
    let Some(resolved) = image.resolve(request.uri().path(), accepted.br, accepted.gzip) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let stream = SquashfsFileStream::new(Arc::clone(&image.fs), resolved.file);
    let content_type = HeaderValue::from_str(resolved.content_type.as_ref())
        .unwrap_or_else(|_| OCTET_STREAM.clone());
    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, content_type);
    headers.insert(ETAG, image.etag.clone().into());
    headers.insert(CACHE_CONTROL, CACHE_NO_CACHE.clone());
    headers.insert(VARY, VARY_ACCEPT_ENCODING.clone());
    headers.insert(X_CONTENT_TYPE_OPTIONS, NOSNIFF.clone());
    if let Some(encoding) = resolved.encoding {
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static(encoding));
    }
    response
}

#[derive(Clone, Copy)]
enum SquashfsState {
    Reading,
    Finishing,
    Done,
}

struct SquashfsFileStream {
    stream: ReceiverStream<Bytes>,
    handle: JoinHandle<io::Result<()>>,
    state: SquashfsState,
}

impl SquashfsFileStream {
    fn new(fs: Arc<FilesystemReader<'static>>, file: SquashfsFileReader) -> Self {
        let (tx, rx) = mpsc::channel::<Bytes>(8);
        let handle = spawn_blocking(move || stream_file(&fs, &file, &tx));
        Self {
            stream: ReceiverStream::new(rx),
            handle,
            state: SquashfsState::Reading,
        }
    }
}

impl Stream for SquashfsFileStream {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            match this.state {
                SquashfsState::Done => return Poll::Ready(None),
                SquashfsState::Reading => match Pin::new(&mut this.stream).poll_next(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Some(bytes)) => return Poll::Ready(Some(Ok(bytes))),
                    Poll::Ready(None) => this.state = SquashfsState::Finishing,
                },
                SquashfsState::Finishing => match Pin::new(&mut this.handle).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(Ok(()))) => {
                        this.state = SquashfsState::Done;
                        return Poll::Ready(None);
                    }
                    Poll::Ready(Ok(Err(e))) => {
                        this.state = SquashfsState::Done;
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Ready(Err(_)) => {
                        this.state = SquashfsState::Done;
                        return Poll::Ready(Some(Err(io::Error::other(
                            "internal error serving file",
                        ))));
                    }
                },
            }
        }
    }
}

fn stream_file(
    fs: &FilesystemReader<'static>,
    file: &SquashfsFileReader,
    tx: &mpsc::Sender<Bytes>,
) -> io::Result<()> {
    let mut reader = FilesystemReaderFile::new(fs, file).reader();
    let block_size = fs.block_size as usize;
    // Reuse one block-sized buffer. `split_to` hands each chunk to the response
    // without copying and keeps the rest of the allocation for the next read.
    let mut buf = BytesMut::new();
    loop {
        if buf.is_empty() {
            buf.resize(block_size, 0);
        }
        match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(read) => {
                if tx.blocking_send(buf.split_to(read).freeze()).is_err() {
                    return Ok(());
                }
            }
            Err(error) => return Err(error),
        }
    }
}

fn placeholder() -> Response {
    let mut response = (StatusCode::SERVICE_UNAVAILABLE, PLACEHOLDER).into_response();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(RETRY_AFTER, HeaderValue::from_static("2"));
    headers.insert(CACHE_CONTROL, CACHE_NO_STORE.clone());
    headers.insert(X_CONTENT_TYPE_OPTIONS, NOSNIFF.clone());
    response
}

/// The `Accept-Encoding` preferences we care about, parsed from a request.
#[derive(Debug, Clone, Copy, Default)]
struct AcceptedEncodings {
    br: bool,
    gzip: bool,
}

impl AcceptedEncodings {
    /// Parses `Accept-Encoding` from `headers`. A missing or unparseable
    /// header yields no accepted encodings.
    fn from_headers(headers: &HeaderMap) -> Self {
        let value = headers
            .get(ACCEPT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let mut out = Self::default();
        for entry in value.split(',') {
            let mut parts = entry.splitn(2, ';');
            let name = parts.next().unwrap_or("").trim();
            let q = parts
                .next()
                .and_then(|p| p.trim().strip_prefix("q="))
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(1.0);
            if q > 0.0 {
                if name.eq_ignore_ascii_case("br") {
                    out.br = true;
                }
                if name.eq_ignore_ascii_case("gzip") {
                    out.gzip = true;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use axum::body::to_bytes;
    use axum::http::HeaderName;
    use axum::http::header::IF_NONE_MATCH;

    use super::*;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/site.sqfs");

    fn image() -> Arc<SpectraImage> {
        Arc::new(SpectraImage::open(Path::new(FIXTURE), &"sha256:abcd".parse().unwrap()).unwrap())
    }

    fn request(uri: &str, headers: &[(HeaderName, &str)]) -> Request {
        let mut builder = Request::builder().uri(uri);
        for (name, value) in headers {
            builder = builder.header(name, *value);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn body_of(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    #[tokio::test]
    async fn serves_index_with_digest_etag() {
        let response = serve(image(), request("/", &[]));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(ETAG).unwrap(), "\"sha256:abcd\"");
        assert!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-cache");
        assert_eq!(
            body_of(response).await,
            b"<!doctype html><title>index</title>"
        );
    }

    #[tokio::test]
    async fn revalidates_with_matching_etag() {
        let response = serve(image(), request("/", &[(IF_NONE_MATCH, "\"sha256:abcd\"")]));
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn negotiates_brotli_variant() {
        let response = serve(
            image(),
            request("/_nuxt/app.js", &[(ACCEPT_ENCODING, "br, gzip")]),
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "br");
        assert!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("javascript")
        );
        assert_eq!(body_of(response).await, b"BROTLI-APP-BYTES");
    }

    #[tokio::test]
    async fn serves_identity_when_no_encoding_accepted() {
        let response = serve(image(), request("/_nuxt/app.js", &[]));
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(CONTENT_ENCODING).is_none());
        assert_eq!(body_of(response).await, b"console.log(\"plain app\")");
    }

    #[tokio::test]
    async fn falls_back_to_spa_shell_for_unknown_route() {
        let response = serve(image(), request("/deep/link", &[]));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            body_of(response).await,
            b"<!doctype html><title>spa-shell</title>"
        );
    }

    #[test]
    fn placeholder_is_a_self_refreshing_page() {
        let response = placeholder();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get(RETRY_AFTER).unwrap(), "2");
        assert!(PLACEHOLDER.contains("refresh"));
    }
}
