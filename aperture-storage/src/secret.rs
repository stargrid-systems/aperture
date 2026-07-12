//! Sensitive hash types with redacting Debug impls.
//!
//! These newtypes wrap the hash values stored in the database. They prevent
//! accidental leakage through Debug formatting while still allowing the
//! underlying bytes to be accessed when needed.

use std::fmt;

use password_hash::PasswordHashString;
use subtle::ConstantTimeEq;
use turso::Value;

use crate::error::{Result, StorageError};
use crate::sql::{FromSql, ToSql};

/// An Argon2 password hash (PHC format string).
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(PasswordHashString);

impl PasswordHash {
    pub fn new(hash: PasswordHashString) -> Self {
        Self(hash)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for PasswordHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PasswordHash").finish_non_exhaustive()
    }
}

impl ToSql for PasswordHash {
    fn to_sql(&self) -> Value {
        Value::Text(self.0.to_string())
    }
}

impl FromSql for PasswordHash {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Text(s) => {
                let parsed = s.parse().map_err(|_| StorageError::ColumnTypeMismatch {
                    column: idx,
                    expected: "valid PHC hash string",
                    actual: Value::Text(s),
                })?;
                Ok(Self(parsed))
            }
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "text",
                actual,
            }),
        }
    }
}

/// SHA-256 hash of a session token.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenHash(Vec<u8>);

impl TokenHash {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TokenHash").finish_non_exhaustive()
    }
}

impl ToSql for TokenHash {
    fn to_sql(&self) -> Value {
        Value::Blob(self.0.clone())
    }
}

impl FromSql for TokenHash {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Blob(bytes) => Ok(Self(bytes)),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "blob",
                actual,
            }),
        }
    }
}

/// SHA-256 hash of an API key.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKeyHash(Vec<u8>);

impl ApiKeyHash {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Constant-time comparison with another hash.
    pub fn matches(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl fmt::Debug for ApiKeyHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ApiKeyHash").finish_non_exhaustive()
    }
}

impl ToSql for ApiKeyHash {
    fn to_sql(&self) -> Value {
        Value::Blob(self.0.clone())
    }
}

impl FromSql for ApiKeyHash {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Blob(bytes) => Ok(Self(bytes)),
            actual => Err(StorageError::ColumnTypeMismatch {
                column: idx,
                expected: "blob",
                actual,
            }),
        }
    }
}
