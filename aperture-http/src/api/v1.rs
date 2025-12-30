use axum::Router;

pub fn router() -> Router {
    Router::new().without_v07_checks()
}
