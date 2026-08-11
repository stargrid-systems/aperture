//! The `os.hostname` setting: runtime hostname configuration.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use aperture_settings::SettingDefinition;
use serde::de::{self, Deserializer};
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
    pub fn new(s: impl Into<Box<str>>) -> Result<Self, HostnameError> {
        let s = s.into();
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
        Self::new(s)
    }
}

impl Serialize for Hostname {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Hostname {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(de::Error::custom)
    }
}

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

/// Setting definition for the `os.hostname` key.
pub struct HostnameDef;

impl SettingDefinition for HostnameDef {
    const KEY: &'static str = "os.hostname";
    type Value = Hostname;
}
