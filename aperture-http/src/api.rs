use axum::Router;

mod v1;

pub fn router() -> Router {
    Router::new()
        .without_v07_checks()
        .nest("/v1", self::v1::router())
}
