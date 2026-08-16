//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra frontend served as a fallback.

use aperture_settings::Settings;
use aperture_storage::{ApiKeyId, ArtifactKey, Storage, UserId};
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
use self::auth::SecurityAddon;
pub use self::avatar::{AvatarAnimation, AvatarStyle};
use self::dto::{JsonQueryString, LevelResponse, OrderParam, TaskStatusParam, VersionSortParam};
pub use self::server::HttpServer;
use self::spectra::fallback as spectra_fallback;
pub use self::spectra::{Spectra, SpectraConfig, SpectraWorker};
pub use self::tls::{
    RegenerateCertificateDefinition, RegenerateCertificateInput, RotateCertificateDefinition,
    install_default_rotation_schedule,
};

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
    components(schemas(JsonQueryString, LevelResponse, OrderParam, TaskStatusParam, VersionSortParam, ApiKeyId, ArtifactKey, UserId)),
    modifiers(&SecurityAddon),
    security(
        ("SessionCookie" = []),
        ("BearerAuth" = [])
    )
)]
struct ApiDoc;
fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api", api_routes())
}

/// Returns the static `OpenAPI` specification of the gateway API.
pub fn openapi() -> OpenApiSpec {
    self::api_router().split_for_parts().1
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

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    /// Operations that need a session but no RBAC permission.
    const SESSION_ONLY: &[&str] = &[
        "getGatewayVersion",
        "listApiKeys",
        "getUserAvatar",
        "logout",
        "getCurrentActor",
        "changePassword",
    ];

    /// Operations that need no authentication at all.
    const PUBLIC: &[&str] = &[
        "login",
        "setup",
        "getSetupStatus",
        "listTaskDefinitions",
        "getTaskDefinition",
        "listSettingDefinitions",
        "getSettingDefinition",
    ];

    #[test]
    fn spec_documents_permissions_per_operation() {
        let spec = serde_json::to_value(openapi()).expect("spec must serialize");
        for (_, item) in spec["paths"].as_object().expect("paths") {
            for method in ["get", "post", "put", "patch", "delete"] {
                let Some(op) = item.get(method) else {
                    continue;
                };
                let op_id = op["operationId"].as_str().expect("operation id").to_owned();
                let is_public = op["security"] == Value::Array(vec![serde_json::json!({})]);

                assert_eq!(
                    is_public,
                    PUBLIC.contains(&op_id.as_str()),
                    "{op_id} public flag mismatch"
                );

                // Looked up by the shared constant, so every annotation
                // literal is pinned to it: a different name fails coverage.
                let Some(permission) = op.get(aperture_auth::REQUIRED_PERMISSION_EXTENSION) else {
                    assert!(
                        is_public || SESSION_ONLY.contains(&op_id.as_str()),
                        "{op_id} documents no required permission"
                    );
                    continue;
                };
                // The values come from Object and Action through
                // required_permission, so the compiler already guarantees the
                // vocabulary. Only the object:action shape needs checking.
                assert!(
                    permission
                        .as_str()
                        .and_then(|perm| perm.split_once(':'))
                        .is_some_and(|(object, action)| !object.is_empty() && !action.is_empty()),
                    "{op_id} permission is not object:action: {permission}"
                );
            }
        }
    }

    /// Collects every `$ref` in `value`, paired with its location as a JSON
    /// path into the spec.
    fn collect_refs(value: &Value, path: &str, refs: &mut Vec<(String, String)>) {
        match value {
            Value::Object(fields) => {
                if let Some(Value::String(reference)) = fields.get("$ref") {
                    refs.push((path.to_owned(), reference.clone()));
                }
                for (name, field) in fields {
                    if name != "$ref" {
                        collect_refs(field, &format!("{path}/{name}"), refs);
                    }
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    collect_refs(item, &format!("{path}/{index}"), refs);
                }
            }
            _ => {}
        }
    }

    /// Resolves a JSON pointer like `components/schemas/Foo` against `spec`.
    fn resolve_pointer<'a>(spec: &'a Value, pointer: &str) -> Option<&'a Value> {
        let mut current = spec;
        for segment in pointer.split('/').filter(|segment| !segment.is_empty()) {
            let segment = segment.replace("~1", "/").replace("~0", "~");
            current = match current {
                Value::Object(map) => map.get(&segment)?,
                Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(current)
    }

    #[test]
    fn spec_references_resolve() {
        let spec = serde_json::to_value(openapi()).expect("spec must serialize");
        let mut refs = Vec::new();
        collect_refs(&spec, "", &mut refs);
        assert!(!refs.is_empty(), "expected the spec to contain references");
        for (path, reference) in &refs {
            let Some(pointer) = reference.strip_prefix("#/") else {
                panic!("external reference at {path}: {reference}");
            };
            assert!(
                resolve_pointer(&spec, pointer).is_some(),
                "dangling reference at {path}: {reference}"
            );
        }
    }

    #[test]
    fn spec_security_requirements_name_registered_schemes() {
        let spec = serde_json::to_value(openapi()).expect("spec must serialize");
        let schemes: Vec<String> = spec["components"]["securitySchemes"]
            .as_object()
            .expect("security schemes")
            .keys()
            .cloned()
            .collect();
        let empty = Vec::new();

        let mut requirements: Vec<&Value> = spec["security"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .collect();
        for (_, item) in spec["paths"].as_object().expect("paths") {
            for method in ["get", "post", "put", "patch", "delete"] {
                if let Some(operation) = item.get(method) {
                    requirements.extend(operation["security"].as_array().unwrap_or(&empty).iter());
                }
            }
        }

        assert!(!requirements.is_empty());
        for requirement in requirements {
            for name in requirement.as_object().expect("requirement").keys() {
                assert!(
                    schemes.contains(name),
                    "security requirement names unknown scheme {name:?}"
                );
            }
        }
    }
}
