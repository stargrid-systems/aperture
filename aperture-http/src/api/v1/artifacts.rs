use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::DownloadResponse;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_downloads))
        .routes(routes!(prefetch))
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

/// Starts a download of the Spectra frontend if it is not already present.
#[utoipa::path(
    post,
    path = "/prefetch",
    responses((status = 202, description = "Prefetch started")),
)]
async fn prefetch(State(state): State<AppState>) -> StatusCode {
    state.spectra().start_prefetch();
    StatusCode::ACCEPTED
}
