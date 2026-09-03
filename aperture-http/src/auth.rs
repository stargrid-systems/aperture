//! Auth middleware, path predicates, session cookie helpers, and `OpenAPI`
//! security scheme registration.

use std::collections::HashSet;
use std::error::Error as StdError;

use aperture_auth::authz::{Permission, Permit};
use aperture_auth::{AuthenticatedActor, RawApiKey, SessionToken};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cookie::time::Duration as CookieDuration;
use cookie::{Cookie, SameSite};
use utoipa::openapi::Components;
use utoipa::openapi::path::Operation;
use utoipa::openapi::security::{
    ApiKey, ApiKeyValue, Http, HttpAuthScheme, SecurityRequirement, SecurityScheme,
};

use crate::error::ApiError;
use crate::{AppState, OpenApiSpec};

/// Extractor that enforces permission `P` and yields the PDP's permit.
///
/// A handler taking `Require<P>` cannot run unless the authenticated
/// subject's roles grant `P`. Rejection is 403, or 401 when no actor is
/// attached (cannot happen behind the auth middleware).
pub struct Require<P>(pub Permit<P>);

impl<P: Permission + 'static> FromRequestParts<AppState> for Require<P> {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = parts
            .extensions
            .get::<AuthenticatedActor>()
            .ok_or(ApiError::UNAUTHORIZED)?;
        let permit = state.auth().require(&auth.subject).await?;
        Ok(Self(permit))
    }
}

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "aperture_session";

/// API paths that skip authentication, derived from the `OpenAPI` document.
///
/// Every operation annotated with `security(())` renders an empty security
/// requirement and marks its path public. Templates without a parameter
/// (for example `/api/v1/auth/login`) become exact matches. Templates with
/// a parameter (for example `/api/v1/task-definitions/{key}`) cover every
/// key below the prefix before the first `{`.
#[derive(Debug, PartialEq, Eq)]
pub struct PublicPaths {
    exact: HashSet<String>,
    prefixes: Vec<String>,
}

impl PublicPaths {
    /// Derives the public paths from the paths of an `OpenAPI` document.
    pub fn from_spec(spec: &OpenApiSpec) -> Self {
        let mut public = Self {
            exact: HashSet::new(),
            prefixes: Vec::new(),
        };
        for (template, item) in &spec.paths.paths {
            let operations = [
                &item.get,
                &item.post,
                &item.put,
                &item.patch,
                &item.delete,
                &item.options,
                &item.head,
                &item.trace,
            ];
            if !operations.into_iter().flatten().any(is_public_operation) {
                continue;
            }
            match template.split_once('{') {
                Some((prefix, _)) => public
                    .prefixes
                    .push(prefix.strip_suffix('/').unwrap_or(prefix).to_owned()),
                None => {
                    public.exact.insert(template.clone());
                }
            }
        }
        public.prefixes.sort();
        public
    }

    /// Builds the set from explicit exact paths and prefixes.
    pub fn new(exact: &[&str], prefixes: &[&str]) -> Self {
        let mut prefixes: Vec<String> =
            prefixes.iter().map(|prefix| (*prefix).to_owned()).collect();
        prefixes.sort();
        Self {
            exact: exact.iter().map(|path| (*path).to_owned()).collect(),
            prefixes,
        }
    }

    /// Returns whether `path` requires no authentication.
    pub fn is_public(&self, path: &str) -> bool {
        self.exact.contains(path) || self.prefixes.iter().any(|prefix| path.starts_with(prefix))
    }
}

/// An operation is public when its only security requirement is the empty
/// one, which is how `security(())` renders.
fn is_public_operation(operation: &Operation) -> bool {
    matches!(
        operation.security.as_deref(),
        Some([requirement]) if *requirement == SecurityRequirement::default()
    )
}

/// Paths accessible when the user must change their password.
fn is_password_change_path(path: &str) -> bool {
    path == "/api/v1/auth/change-password"
        || path == "/api/v1/auth/logout"
        || path == "/api/v1/auth/me"
}

/// Auth middleware: resolves the actor from a session cookie or API key bearer
/// token and stores it in request extensions. Paths the `OpenAPI` document
/// marks public bypass auth, as do the spec endpoint (which the document does
/// not contain) and every non-`/api/` path served by the frontend fallback.
pub async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();

    if !path.starts_with("/api/")
        || path == "/api/openapi.json"
        || state.public_paths().is_public(&path)
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_paths_match_only_themselves() {
        let public = PublicPaths::new(&["/api/v1/auth/login"], &[]);
        assert!(public.is_public("/api/v1/auth/login"));
        assert!(!public.is_public("/api/v1/auth/login/extra"));
        assert!(!public.is_public("/api/v1/auth/logout"));
    }

    #[test]
    fn prefixes_cover_nested_by_key_routes() {
        let public = PublicPaths::new(&[], &["/api/v1/task-definitions"]);
        assert!(public.is_public("/api/v1/task-definitions/download"));
        assert!(public.is_public("/api/v1/task-definitions/download/versions"));
        assert!(!public.is_public("/api/v1/tasks"));
    }

    #[test]
    fn construction_is_order_insensitive() {
        assert_eq!(
            PublicPaths::new(&[], &["/b", "/a"]),
            PublicPaths::new(&[], &["/a", "/b"])
        );
    }
}
