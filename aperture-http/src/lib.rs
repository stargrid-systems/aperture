use std::path::PathBuf;

use axum::Router;

use self::spectra::Spectra;

mod api;
mod spectra;

pub async fn router() -> Router {
    let mut spectra = Spectra::new(PathBuf::from("./xyz"));
    spectra.prep().await.unwrap();

    Router::new()
        .without_v07_checks()
        .nest("/api", self::api::router())
        .fallback_service(spectra.service())
}
