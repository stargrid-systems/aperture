//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra frontend served as a fallback.

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;
use utoipa::openapi::OpenApi as OpenApiSpec;
use utoipa_axum::router::OpenApiRouter;

use self::api::router as api_routes;
use self::spectra::fallback as spectra_fallback;
pub use self::spectra::{Spectra, SpectraConfig};
use aperture_artifacts::LogRepository;

mod api;
mod dto;
mod error;
mod spectra;

/// Shared application state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    version: &'static str,
    spectra: Spectra,
    logs: LogRepository,
}

impl AppState {
    /// Wraps the gateway version, the Spectra frontend, and the log repository
    /// for use as request state.
    pub fn new(version: &'static str, spectra: Spectra, logs: LogRepository) -> Self {
        Self {
            version,
            spectra,
            logs,
        }
    }

    pub(crate) fn version(&self) -> &'static str {
        self.version
    }

    pub(crate) fn spectra(&self) -> &Spectra {
        &self.spectra
    }

    pub(crate) fn logs(&self) -> &LogRepository {
        &self.logs
    }
}

#[derive(OpenApi)]
#[openapi(info(title = "Aperture API"))]
struct ApiDoc;

fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api", api_routes())
}

/// Returns the generated OpenAPI specification for the gateway API.
pub fn openapi() -> OpenApiSpec {
    self::api_router().split_for_parts().1
}

/// Builds the full axum application.
///
/// The JSON API lives under `/api`. Everything else falls back to the Spectra
/// frontend, which the state's [`Spectra`] serves and fetches on demand.
pub fn app(state: AppState) -> Router {
    let (api, doc) = self::api_router().split_for_parts();
    Router::<AppState>::new()
        .merge(api)
        .route("/api/openapi.json", get(move || openapi_doc(doc.clone())))
        .fallback(spectra_fallback)
        .with_state(state)
}

async fn openapi_doc(doc: OpenApiSpec) -> Json<OpenApiSpec> {
    Json(doc)
}
