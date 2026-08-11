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
