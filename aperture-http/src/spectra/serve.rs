//! Serving the current frontend over HTTP, or a placeholder while it installs.

use std::io::{self, Read};
use std::sync::Arc;

use axum::body::{Body, Bytes};
use bytes::BytesMut;
use axum::extract::{Request, State};
use axum::http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
    RETRY_AFTER, VARY,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use backhand::{FilesystemReader, InnerNode};
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;
use tokio_stream::wrappers::ReceiverStream;

use super::image::SpectraImage;
use crate::AppState;

const PLACEHOLDER: &str = include_str!("installing.html");

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
    if matches_etag(request.headers(), &image.etag) {
        return (StatusCode::NOT_MODIFIED, [(ETAG, image.etag.clone())]).into_response();
    }

    let (accept_br, accept_gzip) = accepted_encodings(request.headers());
    let Some(resolved) = image.resolve(request.uri().path(), accept_br, accept_gzip) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let fs = Arc::clone(&image.fs);
    let stored = resolved.stored.clone();
    let (tx, rx) = mpsc::channel::<io::Result<Bytes>>(8);
    spawn_blocking(move || stream_file(&fs, &stored, &tx));

    let content_type = HeaderValue::from_str(resolved.content_type.as_ref())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let mut response = Response::new(Body::from_stream(ReceiverStream::new(rx)));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, content_type);
    headers.insert(ETAG, image.etag.clone());
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Some(encoding) = resolved.encoding {
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static(encoding));
        headers.insert(VARY, HeaderValue::from_static("accept-encoding"));
    }
    response
}

fn stream_file(fs: &FilesystemReader<'static>, stored: &str, tx: &mpsc::Sender<io::Result<Bytes>>) {
    let Some(node) = fs
        .files()
        .find(|node| node.fullpath.to_string_lossy() == stored)
    else {
        return;
    };
    let InnerNode::File(file) = &node.inner else {
        return;
    };
    let mut reader = fs.file(file).reader();
    let block_size = fs.block_size as usize;
    // Reuse one block-sized buffer. `split_to` hands each chunk to the response
    // without copying and keeps the rest of the allocation for the next read.
    let mut buf = BytesMut::new();
    loop {
        if buf.is_empty() {
            buf.resize(block_size, 0);
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                if tx.blocking_send(Ok(buf.split_to(read).freeze())).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = tx.blocking_send(Err(error));
                break;
            }
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
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn matches_etag(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    match headers.get(IF_NONE_MATCH) {
        Some(value) => value == etag || value == "*",
        None => false,
    }
}

fn accepted_encodings(headers: &HeaderMap) -> (bool, bool) {
    let value = headers
        .get(ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    (value.contains("br"), value.contains("gzip"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use axum::body::to_bytes;
    use axum::http::HeaderName;

    use super::*;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/site.sqfs");

    fn image() -> Arc<SpectraImage> {
        Arc::new(SpectraImage::open(Path::new(FIXTURE), "sha256:test").unwrap())
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
        assert_eq!(response.headers().get(ETAG).unwrap(), "\"sha256:test\"");
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
        let response = serve(image(), request("/", &[(IF_NONE_MATCH, "\"sha256:test\"")]));
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
