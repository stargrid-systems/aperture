use axum::Router;
use axum::routing::get;

pub fn router() -> Router {
    Router::new()
        .without_v07_checks()
        .route("version", get(get_version))
}

async fn get_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
