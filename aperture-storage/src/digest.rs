//! Content digest newtype: `algorithm:hex`.

use std::borrow::Cow;
use std::fmt;
use std::result::Result as StdResult;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use turso::Value;
use utoipa::openapi::schema::Type;
use utoipa::openapi::{ObjectBuilder, RefOr, Schema};

use crate::error::{Result, StorageError};
use crate::serde_util::deserialize_from_str;
use crate::sql::{FromSql, ToSql};

/// A content digest, for example `sha256:abc123...`.
///
/// Stored as `algorithm:hex`. The algorithm is currently restricted to
/// `sha256`. Construction validates the format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest {
    algorithm: DigestAlgorithm,
    hex: Box<str>,
}

impl Digest {
    /// Builds a digest from a raw hash byte slice, hex-encoding it.
    pub fn from_hash(algorithm: DigestAlgorithm, hash: &[u8]) -> Self {
        Self {
            algorithm,
            hex: hex::encode(hash).into_boxed_str(),
        }
    }

    /// Returns the digest algorithm.
    pub const fn algorithm(&self) -> DigestAlgorithm {
        self.algorithm
    }

    /// Returns the hex digest without the algorithm prefix.
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl FromStr for Digest {
    type Err = InvalidDigest;

    /// Parses a digest of the form `algorithm:hex`.
    fn from_str(value: &str) -> StdResult<Self, Self::Err> {
        let (algorithm, hex) = value
            .split_once(':')
            .ok_or_else(|| InvalidDigest(value.to_owned()))?;
        let algorithm: DigestAlgorithm = algorithm
            .parse()
            .map_err(|_| InvalidDigest(value.to_owned()))?;
        let valid = hex.len() % 2 == 0 && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            return Err(InvalidDigest(value.to_owned()));
        }
        Ok(Self {
            algorithm,
            hex: hex.to_ascii_lowercase().into_boxed_str(),
        })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { algorithm, hex } = self;
        write!(f, "{algorithm}:{hex}")
    }
}

impl Serialize for Digest {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserialize_from_str(deserializer)
    }
}

impl ToSql for Digest {
    fn to_sql(&self) -> Value {
        Value::Text(self.to_string())
    }
}

impl FromSql for Digest {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Text(s) => {
                Self::from_str(&s).map_err(|_| StorageError::InvalidDigest { raw: s })
            }
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "text",
                actual,
            }),
        }
    }
}

impl utoipa::PartialSchema for Digest {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .description(Some(Cow::Borrowed("Content digest, e.g. `sha256:hex`.")))
            .build()
            .into()
    }
}

impl utoipa::ToSchema for Digest {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("Digest")
    }
}

/// Returned when a digest string fails validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid digest: {0}")]
pub struct InvalidDigest(pub String);

/// Supported digest algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DigestAlgorithm {
    Sha256,
}

impl DigestAlgorithm {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
        }
    }
}

impl FromStr for DigestAlgorithm {
    type Err = InvalidDigest;

    fn from_str(value: &str) -> StdResult<Self, Self::Err> {
        match value {
            "sha256" => Ok(Self::Sha256),
            _ => Err(InvalidDigest(value.to_owned())),
        }
    }
}

impl fmt::Display for DigestAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_str() {
        let d: Digest = "sha256:abc123".parse().unwrap();
        assert_eq!(d.algorithm(), DigestAlgorithm::Sha256);
        assert_eq!(d.hex(), "abc123");
        assert_eq!(d.to_string(), "sha256:abc123");
    }

    #[test]
    fn rejects_missing_colon() {
        assert!("sha256abc".parse::<Digest>().is_err());
    }

    #[test]
    fn rejects_odd_hex() {
        assert!("sha256:abc".parse::<Digest>().is_err());
    }

    #[test]
    fn rejects_non_hex() {
        assert!("sha256:xyz".parse::<Digest>().is_err());
    }

    #[test]
    fn rejects_unknown_algorithm() {
        assert!("md5:abc123".parse::<Digest>().is_err());
    }

    #[test]
    fn normalises_to_lowercase() {
        let d: Digest = "sha256:ABCdef".parse().unwrap();
        assert_eq!(d.hex(), "abcdef");
    }
}
