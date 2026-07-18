//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra frontend served as a fallback.

use aperture_storage::Storage;
use aperture_tasks::{TaskDescriptor, Tasks};
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
use self::dto::{JsonQueryString, LevelResponse, OrderParam, TaskStatusParam, VersionSortParam};
pub use self::server::HttpServer;
use self::spectra::fallback as spectra_fallback;
pub use self::spectra::{Spectra, SpectraConfig};

mod api;
mod conditional;
mod dto;
mod error;
mod server;
mod spectra;
pub mod tls;

/// Shared application state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    version: &'static str,
    boot_id: Uuid,
    storage: Storage,
    spectra: Spectra,
    tasks: Tasks,
}

impl AppState {
    pub fn new(
        version: &'static str,
        boot_id: Uuid,
        storage: Storage,
        spectra: Spectra,
        tasks: Tasks,
    ) -> Self {
        Self {
            version,
            boot_id,
            storage,
            spectra,
            tasks,
        }
    }

    pub(crate) fn version(&self) -> &'static str {
        self.version
    }

    pub(crate) fn boot_id(&self) -> Uuid {
        self.boot_id
    }

    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }

    pub(crate) fn spectra(&self) -> &Spectra {
        &self.spectra
    }

    pub(crate) fn tasks(&self) -> &Tasks {
        &self.tasks
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

/// Builds the full axum application.
///
/// The JSON API lives under `/api`. Everything else falls back to the Spectra
/// frontend, which the state's [`Spectra`] serves and fetches on demand.
/// A [`TraceLayer`] creates a span for each request so per-request tracing
/// shows up in the log viewer.
pub fn app(state: AppState) -> Router {
    let (api, mut doc) = self::api_router().split_for_parts();
    project_tasks(&mut doc, &state.tasks().registry().descriptors());
    Router::<AppState>::new()
        .merge(api)
        .route("/api/openapi.json", get(move || openapi_doc(doc.clone())))
        .fallback(spectra_fallback)
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
