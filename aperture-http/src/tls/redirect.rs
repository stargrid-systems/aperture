//! HTTP-to-HTTPS redirect router.
//!
//! All requests get `307 Temporary Redirect` to HTTPS. Uses 307 (not 308)
//! because the HTTPS port may change at runtime. Requests without a usable
//! `Host` header are rejected with 400.

use axum::Router;
use axum::extract::Request;
use axum::http::uri::PathAndQuery;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

pub fn redirect_router(https_port: u16) -> Router {
    Router::new()
        .fallback(move |request: Request| async move { redirect_to_https(https_port, &request) })
}

fn redirect_to_https(https_port: u16, request: &Request) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(strip_port)
        .filter(|h| !h.bytes().any(|b| b.is_ascii_control() || b == b' '));
    let Some(host) = host else {
        return (
            StatusCode::BAD_REQUEST,
            "redirect requires a usable Host header",
        )
            .into_response();
    };
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", PathAndQuery::as_str);
    let target = if https_port == 443 {
        format!("https://{host}{path}")
    } else {
        format!("https://{host}:{https_port}{path}")
    };
    (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, target)]).into_response()
}

/// Strips `:port` from a Host header. Handles bracketed IPv6.
fn strip_port(host: &str) -> &str {
    if host.starts_with('[')
        && let Some(end) = host.find(']')
    {
        return &host[..=end];
    }
    host.rsplit_once(':').map_or(host, |(name, _)| name)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request as HttpRequest;

    use super::*;

    #[test]
    fn strip_port_handles_bracketed_ipv6() {
        assert_eq!(strip_port("[::1]:8080"), "[::1]");
        assert_eq!(strip_port("[::1]"), "[::1]");
        assert_eq!(strip_port("example.com:8080"), "example.com");
        assert_eq!(strip_port("example.com"), "example.com");
    }

    #[tokio::test]
    async fn rejects_missing_host_header() {
        let req = HttpRequest::builder()
            .uri("/path")
            .body(Body::empty())
            .unwrap();
        let resp = redirect_to_https(8443, &req);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn redirects_with_temporary_redirect() {
        let req = HttpRequest::builder()
            .uri("/path")
            .header("host", "example.com:8080")
            .body(Body::empty())
            .unwrap();
        let resp = redirect_to_https(8443, &req);
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        let location = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "https://example.com:8443/path");
    }

    #[tokio::test]
    async fn omits_port_for_443() {
        let req = HttpRequest::builder()
            .uri("/p")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();
        let resp = redirect_to_https(443, &req);
        let location = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "https://example.com/p");
    }

    #[tokio::test]
    async fn rejects_host_with_whitespace() {
        let req = HttpRequest::builder()
            .uri("/p")
            .header("host", "evil.example other")
            .body(Body::empty())
            .unwrap();
        let resp = redirect_to_https(8443, &req);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rejects_host_with_control_char() {
        let req = HttpRequest::builder()
            .uri("/p")
            .header("host", "evil\t.example")
            .body(Body::empty())
            .unwrap();
        let resp = redirect_to_https(8443, &req);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
