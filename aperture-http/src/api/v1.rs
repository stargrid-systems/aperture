use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use self::artifacts::router as artifacts_routes;
use self::downloads::router as downloads_routes;
use crate::AppState;
use crate::dto::VersionResponse;

mod artifacts;
mod downloads;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_version))
        .nest("/artifacts", artifacts_routes())
        .nest("/downloads", downloads_routes())
}

/// Returns version information about the gateway.
#[utoipa::path(
    get,
    path = "/version",
    responses((status = 200, description = "Gateway version", body = VersionResponse)),
)]
async fn get_version(State(state): State<AppState>) -> Json<VersionResponse> {
    Json(VersionResponse::new(state.version()))
}
