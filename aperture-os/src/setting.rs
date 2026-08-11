//! The `hostname` setting: runtime hostname configuration.

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

/// A validated RFC 1123 hostname, or `None` to use the system default.
///
/// Construction validates the hostname per RFC 1123: 1-253 characters, labels
/// of 1-63 alphanumeric or hyphen characters, no leading or trailing hyphens.
/// An inner value of `None` means "use the OS default hostname."
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Hostname(Option<String>);

impl Hostname {
    /// Creates a `Hostname` holding `name`, validating it per RFC 1123.
    ///
    /// # Errors
    ///
    /// Returns [`HostnameError`] if `name` fails validation.
    pub fn new(name: impl Into<String>) -> Result<Self, HostnameError> {
        let name = name.into();
        validate(&name)?;
        Ok(Self(Some(name)))
    }

    /// Creates a `Hostname` representing "use the system default."
    pub const fn unset() -> Self {
        Self(None)
    }

    /// The validated hostname string, or `None` when unset.
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

fn validate(s: &str) -> Result<(), HostnameError> {
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
    Ok(())
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(name) => f.write_str(name),
            None => f.write_str("(unset)"),
        }
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
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Hostname {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(name) => Self::new(name).map_err(de::Error::custom),
            None => Ok(Self::unset()),
        }
    }
}

impl PartialSchema for Hostname {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .description(Some(Cow::Borrowed(
                "RFC 1123 hostname, or null to use the system default.",
            )))
            .build()
            .into()
    }
}

impl ToSchema for Hostname {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("Hostname")
    }
}

/// Setting definition for the `hostname` key.
pub struct HostnameDef;

impl SettingDefinition for HostnameDef {
    const KEY: &'static str = "hostname";
    type Value = Hostname;

    fn default(&self) -> Self::Value {
        Hostname::unset()
    }
}
