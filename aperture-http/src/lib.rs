//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra frontend served as a fallback.

use aperture_auth::AuthenticatedActor;
use aperture_storage::LogRepository;
use aperture_tasks::{TaskDescriptor, Tasks};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
pub use utoipa::openapi::OpenApi as OpenApiSpec;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{Discriminator, ObjectBuilder, OneOfBuilder, Ref, Schema, Type};
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use self::api::router as api_routes;
use self::api::v1::auth::extract_session_token;
use self::dto::{JsonQueryString, LevelResponse, OrderParam, TaskStatusParam, VersionSortParam};
use self::spectra::fallback as spectra_fallback;
pub use self::spectra::{Spectra, SpectraConfig};

mod api;
mod dto;
mod error;
mod spectra;

/// Shared application state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    version: &'static str,
    boot_id: Uuid,
    spectra: Spectra,
    tasks: Tasks,
    auth: aperture_auth::AuthHandle,
}

impl AppState {
    pub fn new(
        version: &'static str,
        boot_id: Uuid,
        spectra: Spectra,
        tasks: Tasks,
        auth: aperture_auth::AuthHandle,
    ) -> Self {
        Self {
            version,
            boot_id,
            spectra,
            tasks,
            auth,
        }
    }

    pub(crate) fn version(&self) -> &'static str {
        self.version
    }

    pub(crate) fn boot_id(&self) -> Uuid {
        self.boot_id
    }

    pub(crate) fn spectra(&self) -> &Spectra {
        &self.spectra
    }

    pub(crate) fn tasks(&self) -> &Tasks {
        &self.tasks
    }

    pub(crate) fn auth(&self) -> &aperture_auth::AuthHandle {
        &self.auth
    }

    /// Returns the repository over the structured log tables for this request.
    pub(crate) fn logs(&self) -> Result<LogRepository, aperture_storage::StorageError> {
        self.spectra.artifacts().storage().logs()
    }
}

#[derive(OpenApi)]
#[openapi(
    info(title = "Aperture API"),
    // TODO(utoipa): These types are only referenced indirectly as field types
    // of IntoParams structs. utoipa does not discover their schemas
    // automatically. See: <https://github.com/stargrid-systems/aperture/issues/110>.
    components(schemas(JsonQueryString, LevelResponse, OrderParam, TaskStatusParam, VersionSortParam))
)]
struct ApiDoc;

fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api", api_routes())
}

/// Returns the generated OpenAPI specification for the gateway API, with the
/// registered task kinds in `descriptors` projected into it.
pub fn openapi(descriptors: &[TaskDescriptor]) -> OpenApiSpec {
    let mut spec = self::api_router().split_for_parts().1;
    project_tasks(&mut spec, descriptors);
    spec
}

/// Paths that do not require authentication.
fn is_public_path(path: &str) -> bool {
    path == "/api/v1/auth/login" || path == "/api/openapi.json" || !path.starts_with("/api/")
}

/// Paths accessible when the user must change their password.
fn is_password_change_path(path: &str) -> bool {
    path == "/api/v1/auth/change-password" || path == "/api/v1/auth/logout"
}

/// Auth middleware: resolves the actor from a session cookie or API key bearer
/// token and stores it in request extensions. Public paths bypass auth.
async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();

    if is_public_path(&path) {
        return next.run(request).await;
    }

    let actor = match resolve_actor(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    if actor.must_change_password && !is_password_change_path(&path) {
        return StatusCode::FORBIDDEN.into_response();
    }

    request.extensions_mut().insert(actor);
    next.run(request).await
}

/// Tries session cookie first, then API key bearer.
#[allow(clippy::result_large_err)]
async fn resolve_actor(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedActor, Response> {
    if let Some(token) = extract_session_token(headers)
        && let Ok(Some(actor)) = state.auth().resolve_session(&token).await
    {
        return Ok(actor);
    }
    if let Some(key) = extract_bearer_token(headers)
        && let Ok(Some(actor)) = state.auth().resolve_api_key(&key).await
    {
        return Ok(actor);
    }
    Err(StatusCode::UNAUTHORIZED.into_response())
}

/// Extracts the bearer token from the `Authorization` header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(header::AUTHORIZATION)?;
    let value = header.to_str().ok()?;
    value.strip_prefix("Bearer ").map(|t| t.to_owned())
}

/// Builds the full axum application.
pub fn app(state: AppState) -> Router {
    let (api, mut doc) = self::api_router().split_for_parts();
    project_tasks(&mut doc, &state.tasks().registry().descriptors());
    Router::<AppState>::new()
        .merge(api)
        .route("/api/openapi.json", get(move || openapi_doc(doc.clone())))
        .fallback(spectra_fallback)
        .layer(from_fn_with_state(state.clone(), auth_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Projects the registered task kinds into the spec: it adds each kind's input
/// and output component schemas, then a discriminated `CreateTaskInput` union
/// over the per-kind create bodies, and points `POST /tasks` at it.
fn project_tasks(spec: &mut OpenApiSpec, descriptors: &[TaskDescriptor]) {
    if descriptors.is_empty() {
        return;
    }

    let components = spec.components.get_or_insert_with(Default::default);
    let mut union = OneOfBuilder::new().discriminator(Some(Discriminator::new("kind")));
    for descriptor in descriptors {
        for (name, schema) in &descriptor.schemas {
            match components.schemas.get(name) {
                // Two kinds sharing a component name with the same shape (a
                // common type) is fine. A different shape under the same name
                // would silently corrupt the generated client, so fail loudly.
                Some(existing) => assert!(
                    schemas_equal(existing, schema),
                    "task kinds define conflicting OpenAPI schemas for component {name:?}"
                ),
                None => {
                    components.schemas.insert(name.clone(), schema.clone());
                }
            }
        }
        let kind = ObjectBuilder::new()
            .schema_type(Type::String)
            .enum_values(Some([descriptor.kind]))
            .build();
        let variant = ObjectBuilder::new()
            .property("kind", kind)
            .required("kind")
            .property(
                "input",
                Ref::from_schema_name(descriptor.input_name.clone()),
            )
            .required("input")
            .build();
        union = union.item(variant);
    }

    components.schemas.insert(
        "CreateTaskInput".to_owned(),
        Schema::OneOf(union.build()).into(),
    );
    set_create_body(spec);
}

/// Compares two component schemas by their serialized form.
fn schemas_equal(a: &RefOr<Schema>, b: &RefOr<Schema>) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

/// Points the `POST /tasks` request body at the `CreateTaskInput` union.
fn set_create_body(spec: &mut OpenApiSpec) {
    let schema = RefOr::Ref(Ref::from_schema_name("CreateTaskInput"));
    if let Some(item) = spec.paths.paths.get_mut("/api/v1/tasks")
        && let Some(operation) = item.post.as_mut()
        && let Some(body) = operation.request_body.as_mut()
        && let Some(content) = body.content.get_mut("application/json")
    {
        content.schema = Some(schema);
    }
}

async fn openapi_doc(doc: OpenApiSpec) -> Json<OpenApiSpec> {
    Json(doc)
}
