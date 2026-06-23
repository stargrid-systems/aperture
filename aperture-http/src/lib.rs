//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra single-page frontend served as a fallback.

use std::path::PathBuf;
use std::sync::Arc;

use aperture_core::Core;
use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use self::spectra::Spectra;

mod api;
mod dto;
mod spectra;

/// Shared application state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    core: Arc<Core>,
}

impl AppState {
    /// Wraps a core service for use as request state.
    pub fn new(core: Core) -> Self {
        Self {
            core: Arc::new(core),
        }
    }
}

#[derive(OpenApi)]
#[openapi(info(title = "Aperture API"))]
struct ApiDoc;

fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api", self::api::router())
}

/// Returns the generated OpenAPI specification for the gateway API.
pub fn openapi() -> utoipa::openapi::OpenApi {
    self::api_router().split_for_parts().1
}

/// Pre-downloads the Spectra frontend into `<data_dir>/spectra` for offline
/// use.
pub async fn prefetch_spectra(data_dir: PathBuf) -> miette::Result<()> {
    let mut spectra = Spectra::new(data_dir.join("spectra"));
    spectra.prep().await
}

/// Builds the full axum application.
///
/// The JSON API lives under `/api`. Everything else falls back to the Spectra
/// frontend served from `<data_dir>/spectra`. No network access happens here.
/// The frontend is expected to be present locally (see the prefetch command).
pub fn app(state: AppState, data_dir: PathBuf) -> Router {
    let (api, doc) = self::api_router().split_for_parts();
    let spectra = Spectra::new(data_dir.join("spectra"));

    Router::<AppState>::new()
        .merge(api)
        .route("/api/openapi.json", get(move || openapi_doc(doc.clone())))
        .with_state(state)
        .fallback_service(spectra.service())
}

async fn openapi_doc(doc: utoipa::openapi::OpenApi) -> Json<utoipa::openapi::OpenApi> {
    Json(doc)
}
