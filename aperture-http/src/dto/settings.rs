use aperture_storage::ListQuery;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::dto::OrderParam;

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

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
#[into_params(parameter_in = Query)]
pub struct SettingListParams {
    #[param(minimum = 1, maximum = 200, default = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub order: Option<OrderParam>,
}

impl SettingListParams {
    pub fn to_query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
            order: self.order.map(Into::into),
        }
    }
}
