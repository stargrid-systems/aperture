//! HTTP conditional request helpers: `ETag` matching, HTTP date formatting.

use std::time::SystemTime;

use aperture_storage::Digest;
use axum::http::{HeaderMap, HeaderValue, header};
use jiff::Timestamp;

/// An RFC 9110 IMF-fixdate paired with its [`Timestamp`].
///
/// Formats as `Sun, 06 Nov 1994 08:49:37 GMT` via [`HttpDate::as_header`].
#[derive(Debug, Clone, Copy)]
pub struct HttpDate(Timestamp);

impl HttpDate {
    /// Wraps `ts` for HTTP-date formatting.
    pub(crate) const fn from_timestamp(ts: Timestamp) -> Self {
        Self(ts)
    }

    /// Parses an RFC 9110 IMF-fixdate string.
    pub(crate) fn parse(s: &str) -> Result<Self, InvalidHttpDate> {
        let system_time = httpdate::parse_http_date(s).map_err(InvalidHttpDate::ParseError)?;
        let ts = Timestamp::try_from(system_time)
            .map_err(|err| InvalidHttpDate::OutOfRange(err.to_string()))?;
        Ok(Self(ts))
    }

    /// Returns the underlying timestamp.
    pub(crate) const fn to_timestamp(self) -> Timestamp {
        self.0
    }

    /// Formats the timestamp as an IMF-fixdate header value.
    pub(crate) fn as_header(self) -> HeaderValue {
        let system_time: SystemTime = self.0.into();
        HeaderValue::from_str(&httpdate::fmt_http_date(system_time))
            .expect("HTTP date is always valid ASCII")
    }
}

/// Returned when an HTTP-date string fails parsing.
#[derive(Debug, thiserror::Error)]
pub enum InvalidHttpDate {
    /// The string was not a valid IMF-fixdate.
    #[error("invalid HTTP date: {0}")]
    ParseError(#[source] httpdate::Error),
    /// The parsed time is outside the range [`Timestamp`] can represent.
    #[error("HTTP date out of range: {0}")]
    OutOfRange(String),
}

/// A quoted `ETag` suitable for use as an HTTP header value.
#[derive(Debug, Clone)]
pub struct Etag(HeaderValue);

impl Etag {
    /// Wraps an opaque header value as an `ETag`.
    pub(crate) const fn wrap(value: HeaderValue) -> Self {
        Self(value)
    }

    /// Builds a quoted strong `ETag` from a content digest.
    ///
    /// The digest is always valid ASCII (`sha256:hex...`), so this never fails.
    pub(crate) fn from_digest(digest: &Digest) -> Self {
        Self::wrap(HeaderValue::from_str(&format!("\"{digest}\"")).expect("digest is valid ASCII"))
    }

    /// Returns `true` when the request's `If-None-Match` header matches this
    /// `ETag`.
    ///
    /// Handles the wildcard `"*"`, comma-separated lists, and weak validators
    /// (`W/"..."`) per RFC 9110 section 8.8.3.2 (weak comparison algorithm).
    pub(crate) fn matches_if_none_match(&self, headers: &HeaderMap) -> bool {
        let Some(value) = headers.get(header::IF_NONE_MATCH) else {
            return false;
        };
        if value == "*" {
            return true;
        }
        let Ok(raw) = value.to_str() else {
            return false;
        };
        let server_tag = self.0.to_str().unwrap_or("");
        raw.split(',').any(|entry| {
            let entry = entry.trim();
            let entry = entry.strip_prefix("W/").unwrap_or(entry);
            entry == server_tag
        })
    }

    /// Returns `true` when the request indicates a 304 response is
    /// appropriate.
    ///
    /// Evaluates `If-None-Match` first (ETag-based). If absent, falls back to
    /// `If-Modified-Since` (date-based). This precedence is mandated by
    /// RFC 9110 section 13.2.2 (evaluation order).
    pub(crate) fn is_not_modified(&self, headers: &HeaderMap, last_modified: Timestamp) -> bool {
        if headers.contains_key(header::IF_NONE_MATCH) {
            return self.matches_if_none_match(headers);
        }
        if let Some(value) = headers.get(header::IF_MODIFIED_SINCE)
            && let Ok(s) = value.to_str()
            && let Ok(since) = HttpDate::parse(s)
        {
            return last_modified <= since.to_timestamp();
        }
        false
    }
}

impl From<Etag> for HeaderValue {
    fn from(etag: Etag) -> Self {
        etag.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn etag(value: &str) -> Etag {
        Etag::wrap(value.parse().unwrap())
    }

    #[test]
    fn matches_etag_strong_value() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "\"abc\"".parse().unwrap());
        assert!(etag("\"abc\"").matches_if_none_match(&headers));
    }

    #[test]
    fn matches_etag_wildcard() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "*".parse().unwrap());
        assert!(etag("\"abc\"").matches_if_none_match(&headers));
    }

    #[test]
    fn matches_etag_weak_validator() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "W/\"abc\"".parse().unwrap());
        assert!(etag("\"abc\"").matches_if_none_match(&headers));
    }

    #[test]
    fn matches_etag_comma_separated_list() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            "\"def\", W/\"abc\", \"ghi\"".parse().unwrap(),
        );
        assert!(etag("\"abc\"").matches_if_none_match(&headers));
    }

    #[test]
    fn matches_etag_no_match() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "\"xyz\"".parse().unwrap());
        assert!(!etag("\"abc\"").matches_if_none_match(&headers));
    }

    #[test]
    fn matches_etag_missing_header() {
        let headers = HeaderMap::new();
        assert!(!etag("\"abc\"").matches_if_none_match(&headers));
    }

    #[test]
    fn is_not_modified_prefers_etag_over_date() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "\"wrong\"".parse().unwrap());
        let future_date = HttpDate::from_timestamp("2030-01-01T00:00:00Z".parse().unwrap());
        headers.insert(header::IF_MODIFIED_SINCE, future_date.as_header());
        let ts: Timestamp = "2020-01-01T00:00:00Z".parse().unwrap();
        // ETag does not match, so this is not 304, even though If-Modified-Since
        // alone would say 304.
        assert!(!etag("\"abc\"").is_not_modified(&headers, ts));
    }

    #[test]
    fn is_not_modified_falls_back_to_date() {
        let mut headers = HeaderMap::new();
        let future_date = HttpDate::from_timestamp("2030-01-01T00:00:00Z".parse().unwrap());
        headers.insert(header::IF_MODIFIED_SINCE, future_date.as_header());
        let ts: Timestamp = "2020-01-01T00:00:00Z".parse().unwrap();
        assert!(etag("\"abc\"").is_not_modified(&headers, ts));
    }

    #[test]
    fn http_date_parse_returns_error_on_garbage() {
        assert!(HttpDate::parse("not a date").is_err());
    }
}
