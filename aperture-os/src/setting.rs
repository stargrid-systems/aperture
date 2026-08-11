//! The `system` setting scope: runtime hostname configuration.

use aperture_settings::SettingDefinition;
use serde::de::{self, Deserializer};
use serde::Serialize;
use utoipa::ToSchema;

/// Validates a hostname per RFC 1123.
fn validate_hostname(s: &str) -> Result<(), &'static str> {
    if s.is_empty() || s.len() > 253 {
        return Err("hostname must be 1-253 characters long");
    }
    for label in s.split('.') {
        if label.is_empty() {
            return Err("hostname contains an empty label");
        }
        if label.len() > 63 {
            return Err("hostname label exceeds 63 characters");
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("hostname label contains invalid characters");
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("hostname label cannot start or end with a hyphen");
        }
    }
    Ok(())
}

/// The value of the `system` setting scope.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SystemValue {
    /// The hostname to apply to the OS. `None` means use the factory default.
    pub hostname: Option<String>,
}

impl<'de> de::Deserialize<'de> for SystemValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Raw {
            hostname: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        if let Some(ref h) = raw.hostname {
            validate_hostname(h).map_err(de::Error::custom)?;
        }
        Ok(Self {
            hostname: raw.hostname,
        })
    }
}

/// Setting definition for the `system` scope.
pub struct SystemDef;

impl SettingDefinition for SystemDef {
    const KEY: &'static str = "system";
    type Value = SystemValue;

    fn default(&self) -> Self::Value {
        SystemValue { hostname: None }
    }
}
