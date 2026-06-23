use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::VersionResponse;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_version))
}

/// Returns version information about the gateway.
#[utoipa::path(
    get,
    path = "/version",
    responses((status = 200, description = "Gateway version", body = VersionResponse)),
)]
async fn get_version(State(state): State<AppState>) -> Json<VersionResponse> {
    Json(state.core.version().into())
}
