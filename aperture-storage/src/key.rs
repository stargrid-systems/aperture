//! The typed artifact key: a logical identifier for a stored component.
//!
//! Keys are short stable strings, for example `spectra` or `tls/server-cert`.
//! They appear as the `key` column of the artifact catalog and as the path
//! segment of the artifact HTTP API. Wrapping the string in a newtype keeps
//! the call sites honest and centralises validation.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use serde::de::Error as DeError;
use turso::Value;
use utoipa::openapi::schema::Type;
use utoipa::openapi::{ObjectBuilder, RefOr, Schema};
use utoipa::{PartialSchema, ToSchema};

use crate::error::StorageError;
use crate::sql::{FromSql, ToSql};

/// Maximum byte length of an artifact key.
pub const MAX_LEN: usize = 1024;

/// A logical artifact identifier.
///
/// Construct with [`ArtifactKey::new`] (validated) or
/// [`ArtifactKey::from_str`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactKey(Cow<'static, str>);

impl ArtifactKey {
    /// Wraps `key` after validation.
    ///
    /// Rejects empty, absolute paths, path traversal segments (`.` or `..`),
    /// NUL bytes, control characters, and keys longer than 1024 bytes.
    pub fn new(key: impl Into<Cow<'static, str>>) -> Result<Self, InvalidArtifactKey> {
        let key = key.into();
        if key.is_empty() {
            return Err(InvalidArtifactKey::Empty);
        }
        if key.len() > MAX_LEN {
            return Err(InvalidArtifactKey::TooLong);
        }
        if key.starts_with('/') {
            return Err(InvalidArtifactKey::AbsolutePath);
        }
        if key.contains('\0') {
            return Err(InvalidArtifactKey::NulByte);
        }
        if key.bytes().any(|b| b < 0x20 || b == 0x7F) {
            return Err(InvalidArtifactKey::ControlChar);
        }
        for segment in key.split('/') {
            if segment == ".." || segment == "." {
                return Err(InvalidArtifactKey::Traversal);
            }
        }
        Ok(Self(key))
    }

    /// The key as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
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
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ArtifactKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(DeError::custom)
    }
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
    /// The key contained a `.` or `..` segment.
    #[error("artifact key must not contain '.' or '..' segments")]
    Traversal,
    /// The key exceeded 1024 bytes.
    #[error("artifact key must not exceed {MAX_LEN} bytes")]
    TooLong,
}

impl PartialSchema for ArtifactKey {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .max_length(Some(MAX_LEN))
            .description(Some(Cow::Borrowed(
                "Logical artifact identifier, for example `spectra` or `tls/server-cert`.",
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
        assert!(ArtifactKey::new("tls/server-cert").is_ok());
        assert!(ArtifactKey::new("tool/avrdude").is_ok());
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
    fn rejects_traversal() {
        assert_eq!(
            ArtifactKey::new("a/../b").unwrap_err(),
            InvalidArtifactKey::Traversal
        );
        assert_eq!(
            ArtifactKey::new("./a").unwrap_err(),
            InvalidArtifactKey::Traversal
        );
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
    fn rejects_overlong() {
        let s = "a".repeat(MAX_LEN + 1);
        assert_eq!(
            ArtifactKey::new(s).unwrap_err(),
            InvalidArtifactKey::TooLong
        );
    }

    #[test]
    fn roundtrips_through_str() {
        let key = ArtifactKey::new("tls/server-cert").unwrap();
        assert_eq!(key.as_str(), "tls/server-cert");
        assert_eq!(key.to_string(), "tls/server-cert");
    }
}
