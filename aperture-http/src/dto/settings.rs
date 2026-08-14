use aperture_settings::SettingDescriptor;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// One setting key and its current value.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SettingResponse {
    pub key: String,
    pub value: Value,
}

/// Body for `PUT /api/v1/settings/{key}`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateSettingRequest {
    pub value: Value,
}

/// A registered setting key with its JSON Schema.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SettingDefinitionResponse {
    /// The key string.
    pub key: String,
    /// JSON Schema of the key's value.
    pub value_schema: Value,
}

impl From<SettingDescriptor> for SettingDefinitionResponse {
    fn from(descriptor: SettingDescriptor) -> Self {
        Self {
            key: descriptor.key.to_owned(),
            value_schema: serde_json::to_value(&descriptor.value_schema).unwrap_or(Value::Null),
        }
    }
}
