//! Argon2 password hashing and verification.

use std::fmt;

use aperture_storage::PasswordHash;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{
    Error, PasswordHash as PhcHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::{Algorithm, Argon2, Params, Version};
use serde::Deserialize;

use crate::error::AuthError;
use crate::token::random_hex;

/// Minimum password length accepted at setup, creation, and change.
const MIN_PASSWORD_LEN: usize = 12;

/// Returns an Argon2id instance with production-safe default parameters.
fn argon2() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

/// A plaintext password. Redacts in Debug output to avoid accidental leakage.
#[derive(Clone, Deserialize, utoipa::ToSchema)]
#[serde(transparent)]
#[schema(value_type = String)]
pub struct Password(String);

impl Password {
    /// Wraps an existing plaintext string.
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// Returns the underlying plaintext.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Generates a random 32-byte hex password.
    pub fn generate() -> Self {
        Self(random_hex(32))
    }

    /// Validates the password against the minimum length policy.
    pub fn validate(&self) -> Result<(), AuthError> {
        if self.0.len() < MIN_PASSWORD_LEN {
            return Err(AuthError::PasswordTooShort);
        }
        Ok(())
    }

    /// Hashes this password with Argon2id and a random salt.
    pub fn hash(&self) -> Result<PasswordHash, AuthError> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = argon2().hash_password(self.0.as_bytes(), &salt)?;
        Ok(PasswordHash::new(hash.into()))
    }

    /// Verifies this password against a stored hash.
    /// Returns `Ok(true)` on match, `Ok(false)` on mismatch.
    pub fn verify_against(&self, hash: &PasswordHash) -> Result<bool, AuthError> {
        let parsed = PhcHash::new(hash.as_str())?;
        match argon2().verify_password(self.0.as_bytes(), &parsed) {
            Ok(()) => Ok(true),
            Err(Error::Password) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Password").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let password = Password::generate();
        let hash = password.hash().unwrap();
        assert!(password.verify_against(&hash).unwrap());
        assert!(
            !Password::new("wrong".to_owned())
                .verify_against(&hash)
                .unwrap()
        );
    }
}
