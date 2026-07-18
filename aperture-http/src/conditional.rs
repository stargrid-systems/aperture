//! HTTP conditional request helpers: ETag matching, HTTP date formatting.

use std::time::SystemTime;

use axum::http::{HeaderMap, HeaderValue, header};
use jiff::Timestamp;

/// Formats a timestamp as an RFC 7231 IMF-fixdate string.
///
/// Example output: `Sun, 06 Nov 1994 08:49:37 GMT`.
pub(crate) fn format_http_date(ts: Timestamp) -> String {
    let system_time: SystemTime = ts.into();
    httpdate::fmt_http_date(system_time)
}

/// Builds a quoted ETag header value from a content digest.
///
/// The digest is always valid ASCII (`sha256:hex...`), so this never fails.
pub(crate) fn etag_from_digest(digest: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{digest}\"")).expect("digest is valid ASCII")
}

/// Returns `true` when the request's `If-None-Match` header matches `etag` or
/// the wildcard `"*"`.
pub(crate) fn matches_etag(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    match headers.get(header::IF_NONE_MATCH) {
        Some(value) => value == etag || value == "*",
        None => false,
    }
}
