//! Avatar endpoint.

use aperture_auth::AuthenticatedActor;
use aperture_storage::ActorId;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{Response, header};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::{AppState, avatar};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_avatar))
}

/// Returns a deterministic constellation avatar for an actor as inline SVG.
#[utoipa::path(
    get,
    path = "/{actor_id}",
    operation_id = operation_ids::GET_AVATAR,
    params(("actor_id" = ActorId, Path, description = "Actor id")),
    responses(
        (status = 200, description = "Avatar SVG", content_type = "image/svg+xml",
         headers(("Cache-Control" = String, description = "Immutable caching directive"))),
    ),
)]
async fn get_avatar(_auth: AuthenticatedActor, Path(actor_id): Path<ActorId>) -> Response<Body> {
    let svg = avatar::render_svg(actor_id);
    Response::builder()
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from(svg))
        .expect("valid response")
}
