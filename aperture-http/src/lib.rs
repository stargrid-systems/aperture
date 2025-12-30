use axum::Router;

mod api;

pub fn router() -> Router {
    Router::new()
        .without_v07_checks()
        .nest("/api", self::api::router())
}
