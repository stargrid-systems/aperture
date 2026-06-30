use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::{DownloadListParams, DownloadResponse, Page, download_page};
use crate::error::ApiError;

use super::operation_ids;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_downloads))
}

/// Lists download attempts, optionally filtered by status and artifact key.
/// Running attempts carry live byte progress.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_DOWNLOADS,
    params(DownloadListParams),
    responses((status = 200, description = "Downloads", body = Page<DownloadResponse>)),
)]
async fn list_downloads(
    State(state): State<AppState>,
    Query(params): Query<DownloadListParams>,
) -> Result<Json<Page<DownloadResponse>>, ApiError> {
    let artifacts = state.spectra().artifacts();
    let page = artifacts
        .list_downloads(
            params.status.map(Into::into),
            params.key.as_deref(),
            &params.to_query(),
        )
        .await?;

    let live: HashMap<String, _> = artifacts
        .active_downloads()
        .into_iter()
        .map(|progress| (progress.key.clone(), progress))
        .collect();

    Ok(Json(download_page(page, &live)))
}
