use axum::Router;
use axum::http::{StatusCode, Uri};

mod v1;

pub fn router() -> Router {
    Router::new()
        .without_v07_checks()
        .nest("/v1", self::v1::router())
        .fallback(fallback)
}

async fn fallback(_uri: Uri) -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "No route found")
}
