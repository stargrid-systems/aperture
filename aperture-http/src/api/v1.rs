use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use self::api_keys::router as api_keys_routes;
use self::artifacts::router as artifacts_routes;
use self::auth::router as auth_routes;
use self::logs::router as logs_routes;
use self::task_schedules::router as task_schedules_routes;
use self::tasks::{definitions_router as task_definitions_routes, router as tasks_routes};
use self::users::router as users_routes;
use crate::AppState;
use crate::dto::VersionResponse;

mod api_keys;
mod artifacts;
mod auth;
mod logs;
pub(crate) mod operation_ids;
mod task_schedules;
mod tasks;
mod users;

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_gateway_version))
        .nest("/auth", auth_routes())
        .nest("/artifacts", artifacts_routes())
        .nest("/tasks", tasks_routes())
        .nest("/task-definitions", task_definitions_routes())
        .nest("/task-schedules", task_schedules_routes())
        .nest("/logs", logs_routes())
        .nest("/users", users_routes())
        .nest("/api-keys", api_keys_routes())
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
