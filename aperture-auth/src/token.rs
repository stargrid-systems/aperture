//! Session token and API key generation and hashing.

use std::fmt;

use aperture_storage::{ApiKeyHash, TokenHash};
use rand::rngs::OsRng;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Prefix for all API key strings so they are easy to identify.
const API_KEY_PREFIX: &str = "apkey_";

/// Number of characters after the prefix used for database lookup.
const API_KEY_LOOKUP_LEN: usize = 12;

/// A raw session token. Redacts in Debug output.
#[derive(Clone)]
pub struct SessionToken(String);

impl SessionToken {
    /// Wraps an existing token string.
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// Generates a cryptographically random session token (64 hex chars = 32
    /// bytes of entropy).
    pub fn generate() -> Self {
        Self(random_hex(32))
    }

    /// Returns the underlying token string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the SHA-256 hash of this token.
    pub fn hash(&self) -> TokenHash {
        TokenHash::new(sha256(&self.0))
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SessionToken").finish_non_exhaustive()
    }
}

/// A raw API key. Redacts in Debug output.
#[derive(Clone, Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schema", schema(value_type = String))]
pub struct RawApiKey(String);

impl RawApiKey {
    /// Wraps an existing key string.
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// Generates a full API key string (`apkey_` + 48 random hex chars).
    pub fn generate() -> Self {
        Self(format!("{API_KEY_PREFIX}{}", random_hex(24)))
    }

    /// Returns the underlying key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the lookup prefix (first N chars after `apkey_`) for database
    /// lookup, or `None` if the key does not have the expected prefix.
    pub fn lookup_prefix(&self) -> Option<String> {
        let rest = self.0.strip_prefix(API_KEY_PREFIX)?;
        let len = rest.len().min(API_KEY_LOOKUP_LEN);
        Some(rest[..len].to_owned())
    }

    /// Returns the SHA-256 hash of this key.
    pub fn hash(&self) -> ApiKeyHash {
        ApiKeyHash::new(sha256(&self.0))
    }
}

impl fmt::Debug for RawApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RawApiKey").finish_non_exhaustive()
    }
}

fn sha256(input: &str) -> Vec<u8> {
    Sha256::digest(input.as_bytes()).to_vec()
}

pub(crate) fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    {
        use rand::TryRngCore;
        OsRng.try_fill_bytes(&mut buf).expect("OsRng failed");
    }
    let mut s = String::with_capacity(buf.len() * 2);
    for b in &buf {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_tokens_are_unique() {
        let a = SessionToken::generate();
        let b = SessionToken::generate();
        assert_ne!(a.as_str(), b.as_str());
        assert_eq!(a.as_str().len(), 64);
    }

    #[test]
    fn api_keys_have_prefix() {
        let key = RawApiKey::generate();
        assert!(key.as_str().starts_with("apkey_"));
        assert!(key.as_str().len() > API_KEY_PREFIX.len() + 10);
    }

    #[test]
    fn api_key_prefix_extraction() {
        let key = RawApiKey::generate();
        let prefix = key.lookup_prefix().unwrap();
        assert_eq!(prefix.len(), API_KEY_LOOKUP_LEN);
    }

    #[test]
    fn api_key_prefix_rejects_bad_key() {
        let key = RawApiKey::new("not_a_key".to_owned());
        assert!(key.lookup_prefix().is_none());
    }

    #[test]
    fn hash_is_deterministic() {
        let token = SessionToken::new("abc".to_owned());
        let a = token.hash();
        let b = SessionToken::new("abc".to_owned()).hash();
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn hash_produces_32_bytes() {
        let token = SessionToken::new("abc".to_owned());
        assert_eq!(token.hash().as_bytes().len(), 32);
    }
}
