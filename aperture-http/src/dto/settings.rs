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

/// A registered setting definition in a listing.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SettingDefinitionSummary {
    /// The key string.
    pub key: String,
}

/// One registered setting definition, with its full JSON Schema.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SettingDefinitionResponse {
    /// The key string.
    pub key: String,
    /// Standalone JSON Schema (draft 2020-12) of the key's value type.
    pub value_schema: Value,
}
