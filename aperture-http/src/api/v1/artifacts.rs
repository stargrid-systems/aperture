use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::DownloadResponse;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_downloads))
}

/// Lists the downloads currently in flight.
#[utoipa::path(
    get,
    path = "/downloads",
    responses((status = 200, description = "Ongoing downloads", body = [DownloadResponse])),
)]
async fn list_downloads(State(state): State<AppState>) -> Json<Vec<DownloadResponse>> {
    let downloads = state
        .spectra()
        .artifacts()
        .active_downloads()
        .into_iter()
        .map(DownloadResponse::from)
        .collect();
    Json(downloads)
}
