//! Media type newtype with validation.
//!
//! A [`MediaType`] is always safe to use as an HTTP `Content-Type` value.
//! Construction runs the validation rule (bare `type/subtype`, no parameters,
//! no control characters) so callers do not need to re-check.

use std::fmt;
use std::str::FromStr;

/// A content media type, for example `application/vnd.spectra.squashfs`.
///
/// Validated at construction. See [`MediaType::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaType(Box<str>);

impl MediaType {
    /// Returns the media type as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parses `s` as a media type, returning `None` if it is invalid.
    ///
    /// A valid media type is a bare `type/subtype` with no parameters. Both
    /// halves must be RFC 9110 HTTP tokens. Control characters, semicolons
    /// (which would start a parameter list), and commas are rejected.
    pub fn parse(s: &str) -> Option<Self> {
        is_valid(s).then(|| Self(s.into()))
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for MediaType {
    type Err = InvalidMediaType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        is_valid(s).then(|| Self(s.into())).ok_or(InvalidMediaType)
    }
}

/// Returned when a media type fails validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid media type: expected a bare 'type/subtype' with no parameters or control characters"
)]
pub struct InvalidMediaType;

/// Validates `s` as a media type per the rules on [`MediaType::parse`].
fn is_valid(s: &str) -> bool {
    if s.bytes()
        .any(|b| b < 0x20 || b == 0x7F || b == b';' || b == b',')
    {
        return false;
    }
    let Some((ty, sub)) = s.split_once('/') else {
        return false;
    };
    is_token(ty) && is_token(sub)
}

/// RFC 9110 token rule: one or more ASCII alphanumeric or `!#$%&'*+-.^_`|~`.
fn is_token(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bare_type_subtype() {
        assert!(MediaType::parse("application/octet-stream").is_some());
        assert!(MediaType::parse("application/vnd.spectra.squashfs").is_some());
        assert!(MediaType::parse("application/vnd.docker.image.rootfs.diff.tar.gzip").is_some());
    }

    #[test]
    fn rejects_parameters() {
        assert!(MediaType::parse("text/html; charset=utf-8").is_none());
    }

    #[test]
    fn rejects_commas() {
        assert!(MediaType::parse("text/html,text/plain").is_none());
    }

    #[test]
    fn rejects_control_chars() {
        assert!(MediaType::parse("text/html\n").is_none());
        assert!(MediaType::parse("text/html\r").is_none());
        assert!(MediaType::parse("text/ht\tml").is_none());
    }

    #[test]
    fn rejects_missing_slash() {
        assert!(MediaType::parse("text").is_none());
    }

    #[test]
    fn rejects_empty_halves() {
        assert!(MediaType::parse("/html").is_none());
        assert!(MediaType::parse("text/").is_none());
    }

    #[test]
    fn rejects_invalid_token_chars() {
        assert!(MediaType::parse("text/html ").is_none());
        assert!(MediaType::parse("text/<html>").is_none());
    }

    #[test]
    fn from_str_roundtrips() {
        let mt: MediaType = "application/json".parse().unwrap();
        assert_eq!(mt.as_str(), "application/json");
        assert!("text/html; charset=utf-8".parse::<MediaType>().is_err());
    }
}
