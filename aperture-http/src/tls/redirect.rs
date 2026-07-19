//! HTTP-to-HTTPS redirect router.
//!
//! All requests are answered with `307 Temporary Redirect` to the same URL
//! over HTTPS. The HTTPS port is taken from the main listener. If the port is
//! 443, it is omitted from the redirect URL.
//!
//! A `307` is used instead of a `308 Permanent Redirect` because the HTTPS
//! port may be reconfigured at runtime, and a permanent redirect would be
//! cached by clients past the reconfiguration.
//!
//! Requests without a usable `Host` header are rejected with `400 Bad Request`
//! rather than guessed: there is no safe default the gateway can assume.
//!
//! # Open-redirect note
//!
//! The redirect target is built from the verbatim `Host` header the client
//! sends. Browsers will not let users forge Host, so a direct browser request
//! cannot be redirected to an attacker-chosen domain. If anything in front of
//! the gateway (a reverse proxy, a load balancer, a transparent proxy)
//! forwards attacker-controlled Host values, this router will reflect them.
//! Either pin the public hostname in the gateway configuration before
//! exposing the redirect listener behind such a proxy, or strip untrusted
//! Host values at the proxy.

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
        .filter(|h| !h.bytes().any(|b| b == b' ' || b == b'\t'));
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
        .map(|p| p.as_str())
        .unwrap_or("/");
    let target = if https_port == 443 {
        format!("https://{host}{path}")
    } else {
        format!("https://{host}:{https_port}{path}")
    };
    (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, target)]).into_response()
}

/// Strips the `:port` suffix from a Host header value.
///
/// Handles bracketed IPv6 hosts (`[::1]:8080` -> `[::1]`) and bare IPv6
/// addresses without a port (`[::1]` -> `[::1]`).
fn strip_port(host: &str) -> &str {
    if host.starts_with('[')
        && let Some(end) = host.find(']')
    {
        return &host[..=end];
    }
    host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host)
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
        let resp = redirect_to_https(8443, req);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn redirects_with_temporary_redirect() {
        let req = HttpRequest::builder()
            .uri("/path")
            .header("host", "example.com:8080")
            .body(Body::empty())
            .unwrap();
        let resp = redirect_to_https(8443, req);
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
        let resp = redirect_to_https(443, req);
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
        // A Host carrying spaces or tabs is malformed. Reflecting it verbatim
        // into the Location header would produce an invalid URL. Reject
        // instead of guessing what the client meant.
        let req = HttpRequest::builder()
            .uri("/p")
            .header("host", "evil.example other")
            .body(Body::empty())
            .unwrap();
        let resp = redirect_to_https(8443, req);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
