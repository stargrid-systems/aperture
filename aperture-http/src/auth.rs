//! Auth middleware, path predicates, session cookie helpers, and `OpenAPI`
//! security scheme registration.

use std::error::Error as StdError;

use aperture_auth::{AuthenticatedActor, RawApiKey, SessionToken};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cookie::time::Duration as CookieDuration;
use cookie::{Cookie, SameSite};
use utoipa::openapi::Components;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, Http, HttpAuthScheme, SecurityScheme};

use crate::{AppState, OpenApiSpec};

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "aperture_session";

/// API path prefixes that do not require authentication. Prefixes so the
/// nested `/{key}` routes are covered too.
const PUBLIC_API_PREFIXES: &[&str] = &[
    "/api/v1/task-definitions",
    "/api/v1/setting-definitions",
    "/api/v1/event-definitions",
];

/// Paths that do not require authentication.
fn is_public_path(path: &str) -> bool {
    path == "/api/v1/auth/login"
        || path == "/api/v1/auth/setup"
        || path == "/api/v1/auth/setup-status"
        || path == "/api/openapi.json"
        || PUBLIC_API_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
        || !path.starts_with("/api/")
}

/// Paths accessible when the user must change their password.
fn is_password_change_path(path: &str) -> bool {
    path == "/api/v1/auth/change-password"
        || path == "/api/v1/auth/logout"
        || path == "/api/v1/auth/me"
}

/// Auth middleware: resolves the actor from a session cookie or API key bearer
/// token and stores it in request extensions. Public paths bypass auth.
pub async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();

    if is_public_path(&path) {
        return next.run(request).await;
    }

    let actor = match resolve_actor(&state, &headers).await {
        Ok(actor) => actor,
        Err(status) => return status.into_response(),
    };

    if actor.must_change_password && !is_password_change_path(&path) {
        return StatusCode::FORBIDDEN.into_response();
    }

    request.extensions_mut().insert(actor);
    next.run(request).await
}

/// Tries session cookie first, then API key bearer.
async fn resolve_actor(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedActor, StatusCode> {
    if let Some(token) = extract_session_token(headers) {
        match state
            .auth()
            .resolve_session(&SessionToken::new(token))
            .await
        {
            Ok(Some(actor)) => return Ok(actor),
            Ok(None) => {}
            Err(err) => {
                tracing::error!(error = &err as &dyn StdError, "session resolution failed");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }
    if let Some(key) = extract_bearer_token(headers) {
        match state.auth().resolve_api_key(&RawApiKey::new(key)).await {
            Ok(Some(actor)) => return Ok(actor),
            Ok(None) => {}
            Err(err) => {
                tracing::error!(error = &err as &dyn StdError, "api key resolution failed");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

/// Extracts the bearer token from the `Authorization` header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(header::AUTHORIZATION)?;
    let value = header.to_str().ok()?;
    value.strip_prefix("Bearer ").map(str::to_owned)
}

/// Extracts the raw session token from the `Cookie` header, if present.
pub fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::COOKIE)?;
    let header_str = value.to_str().ok()?;
    for pair in header_str.split(';') {
        let pair = pair.trim();
        if let Some(rest) = pair.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            return Some(rest.to_owned());
        }
    }
    None
}

/// Builds a cookie string that sets the session cookie to `token`.
pub fn build_session_cookie(token: &str) -> String {
    Cookie::build((SESSION_COOKIE, token.to_owned()))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(CookieDuration::days(7))
        .build()
        .to_string()
}

/// Builds a cookie string that clears the session cookie.
pub fn clear_session_cookie() -> String {
    Cookie::build((SESSION_COOKIE, ""))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(CookieDuration::seconds(0))
        .build()
        .to_string()
}

/// Adds the session-cookie and bearer-token security schemes to the spec.
///
/// The default security requirement lives on the `#[openapi]` attribute;
/// handlers annotated with `security(())` override it and are documented as
/// public.
pub struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, spec: &mut OpenApiSpec) {
        let components = spec.components.get_or_insert_with(Components::new);
        components.security_schemes.insert(
            "SessionCookie".to_owned(),
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(SESSION_COOKIE))),
        );
        components.security_schemes.insert(
            "BearerAuth".to_owned(),
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
}
