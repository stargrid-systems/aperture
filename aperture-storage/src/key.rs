//! The typed artifact key: a logical identifier for a stored component.
//!
//! Keys are short stable strings, for example `spectra` or `tls_server-cert`.
//! They appear as the `key` column of the artifact catalog and as the path
//! segment of the artifact HTTP API. Wrapping the string in a newtype keeps
//! the call sites honest and centralises validation.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use turso::Value;
use utoipa::openapi::schema::Type;
use utoipa::openapi::{ObjectBuilder, RefOr, Schema};
use utoipa::{PartialSchema, ToSchema};

use crate::error::StorageError;
use crate::serde_util::deserialize_from_str;
use crate::sql::{FromSql, ToSql};

/// Maximum byte length of an artifact key.
pub const MAX_LEN: usize = 1024;

/// A logical artifact identifier.
///
/// Construct with [`ArtifactKey::new`] (validated) or
/// [`ArtifactKey::from_str`].
///
/// Keys are URL-safe: they may only contain `[a-zA-Z0-9._-]`. This guarantees
/// they round-trip through a single HTTP path segment without percent-encoding
/// and never collide with route separators.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactKey(Cow<'static, str>);

impl ArtifactKey {
    /// Wraps `key` after validation.
    ///
    /// Rejects empty, too long, and characters outside `[a-zA-Z0-9._-]`. Also
    /// rejects the literal strings `.` and `..` to prevent path-traversal
    /// confusion in callers that join the key with file paths.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidArtifactKey`] if `key` is empty, too long, or contains
    /// an invalid character.
    pub fn new(key: impl Into<Cow<'static, str>>) -> Result<Self, InvalidArtifactKey> {
        let key = key.into();
        validate(key.as_bytes())?;
        Ok(Self(key))
    }

    /// Wraps a `'static` string after validation, panicking on invalid input.
    ///
    /// Intended for well-known keys declared as `static`. Lets the call site
    /// skip `LazyLock`:
    ///
    /// ```ignore
    /// static SPECTRA: ArtifactKey = ArtifactKey::from_static("spectra");
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `key` fails validation for any reason.
    pub const fn from_static(key: &'static str) -> Self {
        match validate(key.as_bytes()) {
            Ok(()) => Self(Cow::Borrowed(key)),
            Err(InvalidArtifactKey::Empty) => panic!("artifact key is empty"),
            Err(InvalidArtifactKey::AbsolutePath) => panic!("artifact key must not start with '/'"),
            Err(InvalidArtifactKey::NulByte) => panic!("artifact key must not contain NUL bytes"),
            Err(InvalidArtifactKey::ControlChar) => {
                panic!("artifact key must not contain control characters")
            }
            Err(InvalidArtifactKey::Traversal) => {
                panic!("artifact key must not be '.' or '..'")
            }
            Err(InvalidArtifactKey::TooLong) => panic!("artifact key exceeds max length"),
            Err(InvalidArtifactKey::InvalidChar) => {
                panic!("artifact key must be URL-safe [a-zA-Z0-9._-]")
            }
        }
    }

    /// The key as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validates `bytes` as an artifact key.
///
/// Shared between [`ArtifactKey::new`] (fallible) and
/// [`ArtifactKey::from_static`] (panicking). Written as a `const fn` so both
/// paths enforce the same rule from a single source of truth.
const fn validate(bytes: &[u8]) -> Result<(), InvalidArtifactKey> {
    if bytes.is_empty() {
        return Err(InvalidArtifactKey::Empty);
    }
    if bytes.len() > MAX_LEN {
        return Err(InvalidArtifactKey::TooLong);
    }
    if bytes[0] == b'/' {
        return Err(InvalidArtifactKey::AbsolutePath);
    }

    let mut i = 0;
    let mut found_nul = false;
    let mut found_control = false;

    while i < bytes.len() {
        let b = bytes[i];
        if b == 0 {
            found_nul = true;
        } else if b < 0x20 || b == 0x7F {
            found_control = true;
        } else if !is_url_safe(b) {
            return Err(InvalidArtifactKey::InvalidChar);
        }
        i += 1;
    }

    if found_nul {
        return Err(InvalidArtifactKey::NulByte);
    }
    if found_control {
        return Err(InvalidArtifactKey::ControlChar);
    }
    if is_dot(bytes) || is_dot_dot(bytes) {
        return Err(InvalidArtifactKey::Traversal);
    }
    Ok(())
}

/// Whether `b` is in the URL-safe whitelist `[a-zA-Z0-9._-]`.
const fn is_url_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-'
}

/// Whether `bytes` is exactly `b"."`.
const fn is_dot(bytes: &[u8]) -> bool {
    bytes.len() == 1 && bytes[0] == b'.'
}

/// Whether `bytes` is exactly `b".."`.
const fn is_dot_dot(bytes: &[u8]) -> bool {
    bytes.len() == 2 && bytes[0] == b'.' && bytes[1] == b'.'
}

/// Errors returned when an artifact key fails validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidArtifactKey {
    /// The key was empty.
    #[error("artifact key is empty")]
    Empty,
    /// The key started with `/`.
    #[error("artifact key must not start with '/'")]
    AbsolutePath,
    /// The key contained a NUL byte.
    #[error("artifact key must not contain NUL bytes")]
    NulByte,
    /// The key contained an ASCII control character.
    #[error("artifact key must not contain control characters")]
    ControlChar,
    /// The key is exactly `.` or `..`.
    #[error("artifact key must not be '.' or '..'")]
    Traversal,
    /// The key exceeded 1024 bytes.
    #[error("artifact key must not exceed {MAX_LEN} bytes")]
    TooLong,
    /// The key contained a character outside the URL-safe whitelist
    /// `[a-zA-Z0-9._-]`.
    #[error("artifact key must only contain URL-safe chars [a-zA-Z0-9._-]")]
    InvalidChar,
}

impl fmt::Display for ArtifactKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ArtifactKey {
    type Err = InvalidArtifactKey;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_owned())
    }
}

impl TryFrom<String> for ArtifactKey {
    type Error = InvalidArtifactKey;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&'static str> for ArtifactKey {
    type Error = InvalidArtifactKey;

    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl serde::Serialize for ArtifactKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for ArtifactKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserialize_from_str(deserializer)
    }
}

impl PartialSchema for ArtifactKey {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .max_length(Some(MAX_LEN))
            .description(Some(Cow::Borrowed(
                "Logical artifact identifier, for example `spectra` or `tls_server-cert`.",
            )))
            .build()
            .into()
    }
}

impl ToSchema for ArtifactKey {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("ArtifactKey")
    }
}

impl ToSql for ArtifactKey {
    fn to_sql(&self) -> Value {
        Value::Text(self.0.to_string())
    }
}

impl FromSql for ArtifactKey {
    fn from_sql(value: Value, idx: usize) -> Result<Self, StorageError> {
        match value {
            Value::Text(s) => Self::new(s).map_err(StorageError::from),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "text",
                actual,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_keys() {
        assert!(ArtifactKey::new("spectra").is_ok());
        assert!(ArtifactKey::new("tls_server-cert").is_ok());
        assert!(ArtifactKey::new("tool_avrdude").is_ok());
        assert!(ArtifactKey::new("firmware.v2").is_ok());
        assert!(ArtifactKey::new("a").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(ArtifactKey::new("").unwrap_err(), InvalidArtifactKey::Empty);
    }

    #[test]
    fn rejects_absolute() {
        assert_eq!(
            ArtifactKey::new("/etc/passwd").unwrap_err(),
            InvalidArtifactKey::AbsolutePath
        );
    }

    #[test]
    fn rejects_traversal_only_when_whole_key_is_dot_or_dotdot() {
        assert_eq!(
            ArtifactKey::new(".").unwrap_err(),
            InvalidArtifactKey::Traversal
        );
        assert_eq!(
            ArtifactKey::new("..").unwrap_err(),
            InvalidArtifactKey::Traversal
        );
        // Embedding `..` inside a larger key is fine now that `/` is forbidden.
        assert!(ArtifactKey::new("a..b").is_ok());
    }

    #[test]
    fn rejects_control_chars() {
        assert_eq!(
            ArtifactKey::new("a\0b").unwrap_err(),
            InvalidArtifactKey::NulByte
        );
        assert_eq!(
            ArtifactKey::new("a\nb").unwrap_err(),
            InvalidArtifactKey::ControlChar
        );
    }

    #[test]
    fn rejects_url_unsafe_chars() {
        // `/` is the most important rejection. Without it, multi-segment keys
        // would collide with route separators.
        assert_eq!(
            ArtifactKey::new("tls/server-cert").unwrap_err(),
            InvalidArtifactKey::InvalidChar
        );
        assert_eq!(
            ArtifactKey::new("space here").unwrap_err(),
            InvalidArtifactKey::InvalidChar
        );
        assert_eq!(
            ArtifactKey::new("hash#tag").unwrap_err(),
            InvalidArtifactKey::InvalidChar
        );
        assert_eq!(
            ArtifactKey::new("q?query").unwrap_err(),
            InvalidArtifactKey::InvalidChar
        );
        assert_eq!(
            ArtifactKey::new("percent%encoded").unwrap_err(),
            InvalidArtifactKey::InvalidChar
        );
    }

    #[test]
    fn rejects_overlong() {
        let s = "a".repeat(MAX_LEN + 1);
        assert_eq!(
            ArtifactKey::new(s).unwrap_err(),
            InvalidArtifactKey::TooLong
        );
    }

    #[test]
    fn roundtrips_through_str() {
        let key = ArtifactKey::new("tls_server-cert").unwrap();
        assert_eq!(key.as_str(), "tls_server-cert");
        assert_eq!(key.to_string(), "tls_server-cert");
    }
}
