//! The `os.hostname` setting: runtime hostname configuration.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use aperture_settings::SettingDefinition;
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use utoipa::openapi::schema::Type;
use utoipa::openapi::{ObjectBuilder, RefOr, Schema};
use utoipa::{PartialSchema, ToSchema};

use crate::error::HostnameError;

/// A validated RFC 1123 hostname.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hostname(Box<str>);

impl Hostname {
    /// Creates a `Hostname` after validating `s` per RFC 1123.
    ///
    /// # Errors
    ///
    /// Returns [`HostnameError`] if `s` fails validation.
    pub fn new(s: Box<str>) -> Result<Self, HostnameError> {
        if s.is_empty() || s.len() > 253 {
            return Err(HostnameError::InvalidLength);
        }
        for label in s.split('.') {
            if label.is_empty() {
                return Err(HostnameError::EmptyLabel);
            }
            if label.len() > 63 {
                return Err(HostnameError::LabelTooLong);
            }
            if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return Err(HostnameError::InvalidChars);
            }
            if label.starts_with('-') || label.ends_with('-') {
                return Err(HostnameError::HyphenAtEdge);
            }
        }
        Ok(Self(s))
    }

    /// The validated hostname string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Hostname {
    fn default() -> Self {
        // TODO: eventually "aperture-{unique_id}"
        Self(Box::from("aperture"))
    }
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Hostname {
    type Err = HostnameError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.into())
    }
}

// --- Serde ---

impl Serialize for Hostname {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

struct HostnameVisitor;

impl Visitor<'_> for HostnameVisitor {
    type Value = Hostname;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a valid RFC 1123 hostname")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Hostname, E> {
        Hostname::new(v.into()).map_err(E::custom)
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Hostname, E> {
        Hostname::new(v.into_boxed_str()).map_err(E::custom)
    }
}

impl<'de> Deserialize<'de> for Hostname {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(HostnameVisitor)
    }
}

// --- Schema ---

impl PartialSchema for Hostname {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .description(Some(Cow::Borrowed("RFC 1123 hostname.")))
            .build()
            .into()
    }
}

impl ToSchema for Hostname {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("Hostname")
    }
}

/// Setting for the `os.hostname` key: runtime hostname configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct HostnameSetting(Hostname);

impl HostnameSetting {
    /// The validated hostname.
    pub const fn hostname(&self) -> &Hostname {
        &self.0
    }
}

impl SettingDefinition for HostnameSetting {
    const KEY: &'static str = "os.hostname";
}

impl Serialize for HostnameSetting {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for HostnameSetting {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Hostname::deserialize(deserializer).map(Self)
    }
}

impl PartialSchema for HostnameSetting {
    fn schema() -> RefOr<Schema> {
        <Hostname as PartialSchema>::schema()
    }
}

impl ToSchema for HostnameSetting {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("HostnameSetting")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_hostname() {
        let h = Hostname::new(Box::from("aperture")).unwrap();
        assert_eq!(h.as_str(), "aperture");
    }

    #[test]
    fn accepts_dotted_hostname() {
        let h = Hostname::new(Box::from("aperture.local")).unwrap();
        assert_eq!(h.as_str(), "aperture.local");
    }

    #[test]
    fn accepts_hyphen_in_label() {
        Hostname::new(Box::from("a-b")).unwrap();
    }

    #[test]
    fn accepts_single_char_label() {
        Hostname::new(Box::from("a.b.c")).unwrap();
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(
            Hostname::new(Box::from("")).unwrap_err(),
            HostnameError::InvalidLength
        );
    }

    #[test]
    fn rejects_too_long() {
        let s = "a".repeat(254);
        assert_eq!(
            Hostname::new(s.into_boxed_str()).unwrap_err(),
            HostnameError::InvalidLength
        );
    }

    #[test]
    fn rejects_leading_dot() {
        assert_eq!(
            Hostname::new(Box::from(".foo")).unwrap_err(),
            HostnameError::EmptyLabel
        );
    }

    #[test]
    fn rejects_trailing_dot() {
        assert_eq!(
            Hostname::new(Box::from("foo.")).unwrap_err(),
            HostnameError::EmptyLabel
        );
    }

    #[test]
    fn rejects_consecutive_dots() {
        assert_eq!(
            Hostname::new(Box::from("foo..bar")).unwrap_err(),
            HostnameError::EmptyLabel
        );
    }

    #[test]
    fn rejects_label_too_long() {
        let label = "a".repeat(64);
        assert_eq!(
            Hostname::new(label.into_boxed_str()).unwrap_err(),
            HostnameError::LabelTooLong
        );
    }

    #[test]
    fn rejects_invalid_chars() {
        assert_eq!(
            Hostname::new(Box::from("foo_bar")).unwrap_err(),
            HostnameError::InvalidChars
        );
    }

    #[test]
    fn rejects_leading_hyphen() {
        assert_eq!(
            Hostname::new(Box::from("-foo")).unwrap_err(),
            HostnameError::HyphenAtEdge
        );
    }

    #[test]
    fn rejects_trailing_hyphen() {
        assert_eq!(
            Hostname::new(Box::from("foo-")).unwrap_err(),
            HostnameError::HyphenAtEdge
        );
    }

    #[test]
    fn default_is_aperture() {
        assert_eq!(Hostname::default().as_str(), "aperture");
    }
}
