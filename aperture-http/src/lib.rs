//! HTTP layer for the Aperture gateway.
//!
//! Builds the axum application: a versioned JSON API under `/api` plus the
//! Spectra frontend served as a fallback.

use aperture_tasks::{TaskDescriptor, TaskManager};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use utoipa::OpenApi;
use utoipa::openapi::{RefOr, Schema};
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
    tasks: TaskManager,
}

impl AppState {
    /// Wraps the gateway version, the Spectra frontend, and the task manager for
    /// use as request state. `version` is reported by `GET /api/v1/version`.
    pub fn new(version: &'static str, spectra: Spectra, tasks: TaskManager) -> Self {
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

    pub(crate) fn tasks(&self) -> &TaskManager {
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
    let mut variants = Vec::new();
    for descriptor in descriptors {
        for (name, schema) in &descriptor.schemas {
            components
                .schemas
                .entry(name.clone())
                .or_insert_with(|| schema.clone());
        }
        variants.push(json!({
            "type": "object",
            "required": ["kind", "input"],
            "properties": {
                "kind": { "type": "string", "enum": [descriptor.kind] },
                "input": { "$ref": format!("#/components/schemas/{}", descriptor.input_name) },
            },
        }));
    }

    let union = json!({
        "oneOf": variants,
        "discriminator": { "propertyName": "kind" },
    });
    if let Ok(schema) = serde_json::from_value::<RefOr<Schema>>(union) {
        components.schemas.insert("CreateTaskInput".to_owned(), schema);
        set_create_body(spec);
    }
}

/// Points the `POST /tasks` request body at the `CreateTaskInput` union.
fn set_create_body(spec: &mut OpenApiSpec) {
    let Ok(schema) = serde_json::from_value::<RefOr<Schema>>(json!({
        "$ref": "#/components/schemas/CreateTaskInput",
    })) else {
        return;
    };
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
