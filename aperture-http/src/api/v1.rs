use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::dto::VersionResponse;

mod api_keys;
mod artifacts;
mod auth;
mod events;
mod logs;
pub mod operation_ids;
mod settings;
mod task_schedules;
mod tasks;
mod users;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_gateway_version))
        .nest("/auth", auth::router())
        .nest("/artifacts", artifacts::router())
        .nest("/tasks", tasks::router())
        .nest("/task-definitions", tasks::definitions_router())
        .nest("/task-schedules", task_schedules::router())
        .nest("/logs", logs::router())
        .nest("/events", events::router())
        .nest("/event-definitions", events::definitions_router())
        .nest("/settings", settings::router())
        .nest("/setting-definitions", settings::definitions_router())
        .nest("/users", users::router())
        .nest("/api-keys", api_keys::router())
}

/// Returns version information about the gateway.
#[utoipa::path(
    get,
    path = "/version",
    operation_id = operation_ids::GET_GATEWAY_VERSION,
    responses((status = 200, description = "Gateway version", body = VersionResponse)),
)]
async fn get_gateway_version(
    _auth: aperture_auth::AuthenticatedActor,
    State(state): State<AppState>,
) -> Json<VersionResponse> {
    Json(VersionResponse::new(state.version(), state.boot_id()))
}
