//! HTTP conditional request helpers: ETag matching, HTTP date formatting.

use std::time::SystemTime;

use axum::http::{HeaderMap, HeaderValue, header};
use jiff::Timestamp;

/// Formats a timestamp as an RFC 9110 IMF-fixdate header value.
///
/// Example output: `Sun, 06 Nov 1994 08:49:37 GMT`.
pub(crate) fn format_http_date(ts: Timestamp) -> HeaderValue {
    let system_time: SystemTime = ts.into();
    HeaderValue::from_str(&httpdate::fmt_http_date(system_time))
        .expect("HTTP date is always valid ASCII")
}

/// Parses an RFC 9110 IMF-fixdate string into a timestamp.
pub(crate) fn parse_http_date(s: &str) -> Option<Timestamp> {
    let system_time = httpdate::parse_http_date(s).ok()?;
    Timestamp::try_from(system_time).ok()
}

/// Builds a quoted ETag header value from a content digest.
///
/// The digest is always valid ASCII (`sha256:hex...`), so this never fails.
pub(crate) fn etag_from_digest(digest: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{digest}\"")).expect("digest is valid ASCII")
}

/// Returns `true` when the request's `If-None-Match` header matches `etag`.
///
/// Handles the wildcard `"*"`, comma-separated lists, and weak validators
/// (`W/"..."`) per RFC 9110 section 8.8.3.2 (weak comparison algorithm).
pub(crate) fn matches_etag(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    let Some(value) = headers.get(header::IF_NONE_MATCH) else {
        return false;
    };
    if value == "*" {
        return true;
    }
    let Ok(raw) = value.to_str() else {
        return false;
    };
    let server_tag = etag.to_str().unwrap_or("");
    raw.split(',').any(|entry| {
        let entry = entry.trim();
        let entry = entry.strip_prefix("W/").unwrap_or(entry);
        entry == server_tag
    })
}

/// Returns `true` when the request indicates a 304 response is appropriate.
///
/// Evaluates `If-None-Match` first (ETag-based). If absent, falls back to
/// `If-Modified-Since` (date-based). This precedence is mandated by
/// RFC 9110 section 13.2.2 (evaluation order).
pub(crate) fn is_not_modified(
    headers: &HeaderMap,
    etag: &HeaderValue,
    last_modified: Timestamp,
) -> bool {
    if headers.contains_key(header::IF_NONE_MATCH) {
        return matches_etag(headers, etag);
    }
    if let Some(value) = headers.get(header::IF_MODIFIED_SINCE)
        && let Ok(s) = value.to_str()
        && let Some(since) = parse_http_date(s)
    {
        return last_modified <= since;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_etag_strong_value() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "\"abc\"".parse().unwrap());
        let etag = "\"abc\"".parse().unwrap();
        assert!(matches_etag(&headers, &etag));
    }

    #[test]
    fn matches_etag_wildcard() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "*".parse().unwrap());
        let etag = "\"abc\"".parse().unwrap();
        assert!(matches_etag(&headers, &etag));
    }

    #[test]
    fn matches_etag_weak_validator() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "W/\"abc\"".parse().unwrap());
        let etag = "\"abc\"".parse().unwrap();
        assert!(matches_etag(&headers, &etag));
    }

    #[test]
    fn matches_etag_comma_separated_list() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            "\"def\", W/\"abc\", \"ghi\"".parse().unwrap(),
        );
        let etag = "\"abc\"".parse().unwrap();
        assert!(matches_etag(&headers, &etag));
    }

    #[test]
    fn matches_etag_no_match() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "\"xyz\"".parse().unwrap());
        let etag = "\"abc\"".parse().unwrap();
        assert!(!matches_etag(&headers, &etag));
    }

    #[test]
    fn matches_etag_missing_header() {
        let headers = HeaderMap::new();
        let etag = "\"abc\"".parse().unwrap();
        assert!(!matches_etag(&headers, &etag));
    }

    #[test]
    fn is_not_modified_prefers_etag_over_date() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "\"wrong\"".parse().unwrap());
        let future_date = format_http_date("2030-01-01T00:00:00Z".parse().unwrap());
        headers.insert(header::IF_MODIFIED_SINCE, future_date);
        let etag = "\"abc\"".parse().unwrap();
        let ts: Timestamp = "2020-01-01T00:00:00Z".parse().unwrap();
        // ETag does not match, so this is not 304, even though If-Modified-Since
        // alone would say 304.
        assert!(!is_not_modified(&headers, &etag, ts));
    }

    #[test]
    fn is_not_modified_falls_back_to_date() {
        let mut headers = HeaderMap::new();
        let future_date = format_http_date("2030-01-01T00:00:00Z".parse().unwrap());
        headers.insert(header::IF_MODIFIED_SINCE, future_date);
        let etag = "\"abc\"".parse().unwrap();
        let ts: Timestamp = "2020-01-01T00:00:00Z".parse().unwrap();
        assert!(is_not_modified(&headers, &etag, ts));
    }
}

