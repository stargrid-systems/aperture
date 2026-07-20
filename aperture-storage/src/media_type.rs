//! Media type newtype with validation.
//!
//! A [`MediaType`] is always safe to use as an HTTP `Content-Type` value.
//! Construction runs the validation rule (bare `type/subtype`, no parameters,
//! no control characters) so callers do not need to re-check.

use std::borrow::Cow;
use std::fmt;
use std::result::Result as StdResult;
use std::str::FromStr;

use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use turso::Value;
use utoipa::openapi::schema::Type;
use utoipa::openapi::{ObjectBuilder, RefOr, Schema};
use utoipa::{PartialSchema, ToSchema};

use crate::error::{Result, StorageError};
use crate::sql::{FromSql, ToSql};

/// A content media type, for example `application/vnd.spectra.squashfs`.
///
/// Validated at construction via [`FromStr`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MediaType(Box<str>);

impl MediaType {
    /// Wraps `s` after validation. Returns the validated media type, or
    /// [`InvalidMediaType`] when `s` is not a bare `type/subtype`.
    pub fn new(s: &str) -> StdResult<Self, InvalidMediaType> {
        s.parse()
    }

    /// Returns the media type as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for MediaType {
    type Err = InvalidMediaType;

    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        is_valid(s).then(|| Self(s.into())).ok_or(InvalidMediaType)
    }
}

impl Serialize for MediaType {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for MediaType {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(DeError::custom)
    }
}

impl ToSql for MediaType {
    fn to_sql(&self) -> Value {
        Value::Text(self.to_string())
    }
}

impl FromSql for MediaType {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Text(s) => {
                Self::from_str(&s).map_err(|_| StorageError::InvalidMediaType { raw: s })
            }
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "text",
                actual,
            }),
        }
    }
}

impl PartialSchema for MediaType {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .description(Some(Cow::Borrowed(
                "A bare 'type/subtype' media type with no parameters.",
            )))
            .build()
            .into()
    }
}

impl ToSchema for MediaType {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("MediaType")
    }
}

/// Returned when a media type fails validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid media type: expected a bare 'type/subtype' with no parameters or control characters"
)]
pub struct InvalidMediaType;

/// Validates `s` as a media type per the rules on [`MediaType`].
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
        assert!("application/octet-stream".parse::<MediaType>().is_ok());
        assert!(
            "application/vnd.spectra.squashfs"
                .parse::<MediaType>()
                .is_ok()
        );
        assert!(
            "application/vnd.docker.image.rootfs.diff.tar.gzip"
                .parse::<MediaType>()
                .is_ok()
        );
    }

    #[test]
    fn rejects_parameters() {
        assert!("text/html; charset=utf-8".parse::<MediaType>().is_err());
    }

    #[test]
    fn rejects_commas() {
        assert!("text/html,text/plain".parse::<MediaType>().is_err());
    }

    #[test]
    fn rejects_control_chars() {
        assert!("text/html\n".parse::<MediaType>().is_err());
        assert!("text/html\r".parse::<MediaType>().is_err());
        assert!("text/ht\tml".parse::<MediaType>().is_err());
    }

    #[test]
    fn rejects_missing_slash() {
        assert!("text".parse::<MediaType>().is_err());
    }

    #[test]
    fn rejects_empty_halves() {
        assert!("/html".parse::<MediaType>().is_err());
        assert!("text/".parse::<MediaType>().is_err());
    }

    #[test]
    fn rejects_invalid_token_chars() {
        assert!("text/html ".parse::<MediaType>().is_err());
        assert!("text/<html>".parse::<MediaType>().is_err());
    }
}
