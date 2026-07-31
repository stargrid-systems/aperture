//! The `Username` newtype: a login name validated by construction.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::AuthError;

/// Minimum and maximum length of a username.
const MIN_LEN: usize = 1;
const MAX_LEN: usize = 64;

/// A login name that is guaranteed valid by construction.
///
/// Allowed characters are ASCII letters, digits, and `_`, `-`, `.`. The length
/// is bounded to 1..=64 characters. Because validation runs in [`TryFrom`] (and
/// therefore during deserialization), a request body with an invalid username
/// is rejected at the extractor boundary with a `400` before any handler runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(try_from = "String", into = "String")]
#[schema(value_type = String)]
pub struct Username(String);

impl Username {
    /// Returns the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Username {
    type Err = AuthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl TryFrom<&str> for Username {
    type Error = AuthError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        // Byte length is intentional: only ASCII chars pass below, so bytes == chars.
        if value.len() < MIN_LEN || value.len() > MAX_LEN {
            return Err(AuthError::InvalidUsername);
        }
        if !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(AuthError::InvalidUsername);
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for Username {
    type Error = AuthError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl From<Username> for String {
    fn from(username: Username) -> Self {
        username.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_usernames() {
        assert!(Username::try_from("admin".to_owned()).is_ok());
        assert!(Username::try_from("alice.bob-2".to_owned()).is_ok());
        assert!(Username::try_from("a".to_owned()).is_ok());
    }

    #[test]
    fn rejects_empty_and_overlong() {
        assert!(Username::try_from(String::new()).is_err());
        assert!(Username::try_from("x".repeat(65)).is_err());
    }

    #[test]
    fn rejects_disallowed_characters() {
        assert!(Username::try_from("bad name".to_owned()).is_err());
        assert!(Username::try_from("tab\there".to_owned()).is_err());
        assert!(Username::try_from("slash/here".to_owned()).is_err());
        assert!(Username::try_from("unicod\u{e9}".to_owned()).is_err());
    }
}
