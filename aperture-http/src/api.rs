use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

pub mod v1;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().nest("/v1", self::v1::router())
}
