//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra frontend served as a fallback.

use aperture_settings::Settings;
use aperture_storage::{ApiKeyId, Storage, UserId};
use aperture_tasks::Tasks;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::{Json, Router};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
pub use utoipa::openapi::OpenApi as OpenApiSpec;
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use self::api::router as api_routes;
pub use self::avatar::{AvatarAnimation, AvatarStyle};
use self::dto::{JsonQueryString, LevelResponse, OrderParam, TaskStatusParam, VersionSortParam};
pub use self::server::HttpServer;
use self::spectra::fallback as spectra_fallback;
pub use self::spectra::{Spectra, SpectraConfig, SpectraWorker};
pub use self::tls::{RotateCertificateDefinition, install_default_rotation_schedule};

mod api;
mod auth;
mod avatar;
mod conditional;
mod dto;
mod error;
mod server;
mod spectra;
mod tls;

/// Shared application state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    version: &'static str,
    boot_id: Uuid,
    storage: Storage,
    spectra: Spectra,
    tasks: Tasks,
    settings: Settings,
    auth: aperture_auth::AuthHandle,
    login_limiter: aperture_auth::LoginLimiter,
}

impl AppState {
    pub fn new(
        version: &'static str,
        boot_id: Uuid,
        storage: Storage,
        spectra: Spectra,
        tasks: Tasks,
        settings: Settings,
        auth: aperture_auth::AuthHandle,
    ) -> Self {
        Self {
            version,
            boot_id,
            storage,
            spectra,
            tasks,
            settings,
            auth,
            login_limiter: aperture_auth::LoginLimiter::default(),
        }
    }

    pub(crate) const fn version(&self) -> &'static str {
        self.version
    }

    pub(crate) const fn boot_id(&self) -> Uuid {
        self.boot_id
    }

    pub(crate) const fn storage(&self) -> &Storage {
        &self.storage
    }

    pub(crate) const fn spectra(&self) -> &Spectra {
        &self.spectra
    }

    pub(crate) const fn tasks(&self) -> &Tasks {
        &self.tasks
    }

    pub(crate) const fn settings(&self) -> &Settings {
        &self.settings
    }

    pub(crate) const fn auth(&self) -> &aperture_auth::AuthHandle {
        &self.auth
    }

    pub(crate) const fn login_limiter(&self) -> &aperture_auth::LoginLimiter {
        &self.login_limiter
    }
}

#[derive(OpenApi)]
#[openapi(
    info(title = "Aperture API"),
    // TODO(utoipa): These types are only referenced indirectly as field types
    // of IntoParams structs. utoipa does not discover their schemas
    // automatically. See: <https://github.com/stargrid-systems/aperture/issues/110>.
    components(schemas(JsonQueryString, LevelResponse, OrderParam, TaskStatusParam, VersionSortParam, ApiKeyId, UserId))
)]
struct ApiDoc;

fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api", api_routes())
}

/// Returns the static `OpenAPI` specification of the gateway API.
///
/// The spec describes the API surface itself; the per-definition JSON Schemas
/// live behind the definitions endpoints instead, so the spec stays a stable
/// contract for code generation.
pub fn openapi() -> OpenApiSpec {
    let mut spec = self::api_router().split_for_parts().1;
    auth::add_security_schemes(&mut spec);
    spec
}

/// Builds the full axum application.
///
/// The JSON API lives under `/api`. Everything else falls back to the Spectra
/// frontend, which the state's [`Spectra`] serves and fetches on demand.
/// A [`TraceLayer`] creates a span for each request so per-request tracing
/// shows up in the log viewer.
pub fn app(state: AppState) -> Router {
    let (api, doc) = self::api_router().split_for_parts();
    Router::<AppState>::new()
        .merge(api)
        .route(
            "/api/openapi.json",
            get(move || {
                let doc = doc.clone();
                async move { Json(doc) }
            }),
        )
        .fallback(spectra_fallback)
        .layer(from_fn_with_state(state.clone(), auth::auth_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
