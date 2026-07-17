//! HTTP-to-HTTPS redirect router.
//!
//! All requests are answered with `308 Permanent Redirect` to the same URL
//! over HTTPS. The HTTPS port is taken from the main listener. If the port is
//! 443, it is omitted from the redirect URL.

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// Builds a router that redirects every request to HTTPS.
pub fn redirect_router(https_port: u16) -> Router {
    Router::new()
        .fallback(move |request: Request| async move { redirect_to_https(https_port, request) })
}

fn redirect_to_https(https_port: u16, request: Request) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(strip_port)
        .unwrap_or("localhost");
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let target = if https_port == 443 {
        format!("https://{host}{path}")
    } else {
        format!("https://{host}:{https_port}{path}")
    };
    (StatusCode::PERMANENT_REDIRECT, [(header::LOCATION, target)]).into_response()
}

fn strip_port(host: &str) -> &str {
    host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host)
}
