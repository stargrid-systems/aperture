//! Session token and API key generation and hashing.

use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

/// Prefix for all API key strings so they are easy to identify.
const API_KEY_PREFIX: &str = "apkey_";

/// Number of characters after the prefix used for database lookup.
const API_KEY_LOOKUP_LEN: usize = 12;

/// Generates a cryptographically random session token (64 hex chars = 32 bytes
/// of entropy).
pub fn generate_session_token() -> String {
    let bytes = random_bytes(32);
    hex_encode(&bytes)
}

/// Generates a full API key string (`apkey_` + 48 random hex chars).
pub fn generate_api_key() -> String {
    let bytes = random_bytes(24);
    format!("{API_KEY_PREFIX}{}", hex_encode(&bytes))
}

/// Returns the lookup prefix (first N chars after `apkey_`) for a full key, or
/// `None` if the key does not have the expected prefix.
pub fn api_key_lookup_prefix(key: &str) -> Option<String> {
    let rest = key.strip_prefix(API_KEY_PREFIX)?;
    let len = rest.len().min(API_KEY_LOOKUP_LEN);
    Some(rest[..len].to_owned())
}

/// SHA-256 hash of a token or key, hex-encoded. The raw value is never stored.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex_encode(&digest)
}

/// Constant-time comparison of two hex strings.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    subtle::ConstantTimeEq::ct_eq(a.as_bytes(), b.as_bytes()).into()
}

fn random_bytes(len: usize) -> Vec<u8> {
    use rand::TryRngCore;
    let mut buf = vec![0u8; len];
    OsRng
        .try_fill_bytes(&mut buf)
        .expect("OsRng failed");
    buf
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_tokens_are_unique() {
        let a = generate_session_token();
        let b = generate_session_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn api_keys_have_prefix() {
        let key = generate_api_key();
        assert!(key.starts_with("apkey_"));
        assert!(key.len() > API_KEY_PREFIX.len() + 10);
    }

    #[test]
    fn api_key_prefix_extraction() {
        let key = generate_api_key();
        let prefix = api_key_lookup_prefix(&key).unwrap();
        assert_eq!(prefix.len(), API_KEY_LOOKUP_LEN);
    }

    #[test]
    fn api_key_prefix_rejects_bad_key() {
        assert!(api_key_lookup_prefix("not_a_key").is_none());
    }

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(hash_token("abc"), hash_token("abc"));
        assert_ne!(hash_token("abc"), hash_token("abd"));
    }
}
