use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

pub(crate) mod v1;

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().nest("/v1", self::v1::router())
}
