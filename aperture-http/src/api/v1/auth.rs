use aperture_auth::AuthenticatedActor;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use cookie::time::Duration as CookieDuration;
use cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::error::ApiError;

/// Name of the session cookie.
const SESSION_COOKIE: &str = "aperture_session";

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(change_password))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    must_change_password: bool,
}

/// Authenticates a user and creates a session.
#[utoipa::path(
    post,
    path = "/login",
    operation_id = operation_ids::LOGIN,
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 401, description = "Invalid credentials"),
    ),
)]
async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let result = state
        .auth()
        .login(&request.username, &request.password)
        .await?;
    let cookie = Cookie::build((SESSION_COOKIE, result.token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(CookieDuration::days(7))
        .build();
    let mut response = Json(LoginResponse {
        must_change_password: result.must_change_password,
    })
    .into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie.to_string().parse().expect("valid header value"),
    );
    Ok(response)
}

/// Destroys the current session.
#[utoipa::path(
    post,
    path = "/logout",
    operation_id = operation_ids::LOGOUT,
    responses((status = 204, description = "Logged out")),
)]
async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = extract_session_token(&headers) {
        state.auth().delete_session(&token).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        clear_session_cookie().parse().expect("valid header value"),
    );
    Ok(response)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// Changes the password for the authenticated user.
#[utoipa::path(
    post,
    path = "/change-password",
    operation_id = operation_ids::CHANGE_PASSWORD,
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "Password changed"),
        (status = 401, description = "Current password is incorrect"),
    ),
)]
async fn change_password(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let users = state.auth().storage().users()?;
    let user = users
        .find_by_actor_id(auth.actor_id())
        .await?
        .ok_or(ApiError::FORBIDDEN)?;
    if !aperture_auth::verify_password(&request.current_password, &user.password_hash)? {
        return Err(ApiError::UNAUTHORIZED);
    }
    state.auth().change_password(user.id, &request.new_password).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Extracts the raw session token from the `Cookie` header, if present.
pub(crate) fn extract_session_token(headers: &HeaderMap) -> Option<String> {
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

/// Sets a removal cookie for the session.
pub(crate) fn clear_session_cookie() -> String {
    Cookie::build((SESSION_COOKIE, ""))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(CookieDuration::seconds(0))
        .build()
        .to_string()
}
