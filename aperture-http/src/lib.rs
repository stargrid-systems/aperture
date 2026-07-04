//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra frontend served as a fallback.

use aperture_tasks::{TaskDescriptor, Tasks};
use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{Discriminator, ObjectBuilder, OneOfBuilder, Ref, Schema, Type};
pub use utoipa::openapi::OpenApi as OpenApiSpec;
use utoipa_axum::router::OpenApiRouter;

use self::api::router as api_routes;
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
    spectra: Spectra,
    tasks: Tasks,
}

impl AppState {
    /// Wraps the gateway version, the Spectra frontend, and the task manager for
    /// use as request state. `version` is reported by `GET /api/v1/version`.
    pub fn new(version: &'static str, spectra: Spectra, tasks: Tasks) -> Self {
        Self {
            version,
            spectra,
            tasks,
        }
    }

    pub(crate) fn version(&self) -> &'static str {
        self.version
    }

    pub(crate) fn spectra(&self) -> &Spectra {
        &self.spectra
    }

    pub(crate) fn tasks(&self) -> &Tasks {
        &self.tasks
    }
}

#[derive(OpenApi)]
#[openapi(info(title = "Aperture API"))]
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
pub fn app(state: AppState) -> Router {
    let (api, mut doc) = self::api_router().split_for_parts();
    project_tasks(&mut doc, &state.tasks().registry().descriptors());
    Router::<AppState>::new()
        .merge(api)
        .route("/api/openapi.json", get(move || openapi_doc(doc.clone())))
        .fallback(spectra_fallback)
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
            components
                .schemas
                .entry(name.clone())
                .or_insert_with(|| schema.clone());
        }
        let kind = ObjectBuilder::new()
            .schema_type(Type::String)
            .enum_values(Some([descriptor.kind]))
            .build();
        let variant = ObjectBuilder::new()
            .property("kind", kind)
            .required("kind")
            .property("input", Ref::from_schema_name(descriptor.input_name.clone()))
            .required("input")
            .build();
        union = union.item(variant);
    }

    components
        .schemas
        .insert("CreateTaskInput".to_owned(), Schema::OneOf(union.build()).into());
    set_create_body(spec);
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
