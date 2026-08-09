use std::future::Future;

use aperture_auth::{AuthenticatedActor, Password, Role, SessionToken, Username};
use aperture_storage::ActorId;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::auth::{build_session_cookie, clear_session_cookie, extract_session_token};
use crate::error::ApiError;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(current_user))
        .routes(routes!(change_password))
        .routes(routes!(setup_status))
        .routes(routes!(setup))
        .layer(RequestBodyLimitLayer::new(64 * 1024))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    username: Username,
    password: Password,
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
    security(()),
)]
async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let result = throttled_auth(state.login_limiter(), request.username.as_str(), async {
        state
            .auth()
            .login(&request.username, &request.password)
            .await
            .map(Some)
    })
    .await?;
    Ok(build_login_response(&result))
}

/// Destroys the current session.
#[utoipa::path(
    post,
    path = "/logout",
    operation_id = operation_ids::LOGOUT,
    responses((status = 204, description = "Logged out")),
)]
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = extract_session_token(&headers) {
        state
            .auth()
            .delete_session(&SessionToken::new(token))
            .await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        clear_session_cookie().parse().expect("valid header value"),
    );
    Ok(response)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentUserResponse {
    actor_id: ActorId,
    username: Option<String>,
    display_name: String,
    roles: Vec<Role>,
    must_change_password: bool,
}

/// Returns the authenticated caller's identity and role.
#[utoipa::path(
    get,
    path = "/me",
    operation_id = operation_ids::GET_CURRENT_USER,
    responses((status = 200, description = "Current caller", body = CurrentUserResponse)),
)]
async fn current_user(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
) -> Result<Json<CurrentUserResponse>, ApiError> {
    let username = if auth.actor.kind == aperture_storage::ActorKind::User {
        state
            .storage()
            .users()?
            .find_by_actor_id(auth.actor.id)
            .await?
            .map(|u| u.username)
    } else {
        None
    };
    let roles = state.auth().roles_for(&auth.subject).await?;
    Ok(Json(CurrentUserResponse {
        actor_id: auth.actor.id,
        username,
        display_name: auth.actor.display_name,
        roles,
        must_change_password: auth.must_change_password,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    current_password: Password,
    new_password: Password,
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
        (status = 403, description = "API-key authenticated callers cannot change a password"),
    ),
)]
async fn change_password(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let keep = extract_session_token(&headers).map(|t| SessionToken::new(t).hash());
    if keep.is_none() {
        return Err(ApiError::FORBIDDEN);
    }
    state
        .auth()
        .change_password(
            auth.actor.id,
            &request.current_password,
            &request.new_password,
            keep.as_ref(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SetupStatusResponse {
    setup_required: bool,
}

/// Returns whether initial setup (creating the first admin user) is needed.
#[utoipa::path(
    get,
    path = "/setup-status",
    operation_id = operation_ids::SETUP_STATUS,
    responses((status = 200, description = "Setup status", body = SetupStatusResponse)),
    security(()),
)]
async fn setup_status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatusResponse>, ApiError> {
    let setup_required = state.auth().is_setup_required().await?;
    Ok(Json(SetupStatusResponse { setup_required }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetupRequest {
    username: Username,
    password: Password,
}

/// Creates the initial admin user. Only available when no users exist.
#[utoipa::path(
    post,
    path = "/setup",
    operation_id = operation_ids::SETUP,
    request_body = SetupRequest,
    responses(
        (status = 200, description = "Setup complete, session created", body = LoginResponse),
        (status = 409, description = "Setup already complete"),
    ),
    security(()),
)]
async fn setup(
    State(state): State<AppState>,
    Json(request): Json<SetupRequest>,
) -> Result<Response, ApiError> {
    let result = throttled_auth(
        state.login_limiter(),
        request.username.as_str(),
        state
            .auth()
            .setup_admin(&request.username, &request.password),
    )
    .await?;
    Ok(build_login_response(&result))
}

fn build_login_response(result: &aperture_auth::LoginResult) -> Response {
    let cookie = build_session_cookie(result.token.as_str());
    let mut response = Json(LoginResponse {
        must_change_password: result.must_change_password,
    })
    .into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie.parse().expect("valid header value"),
    );
    response
}

async fn throttled_auth(
    limiter: &aperture_auth::LoginLimiter,
    username: &str,
    auth_call: impl Future<
        Output = Result<Option<aperture_auth::LoginResult>, aperture_auth::AuthError>,
    >,
) -> Result<aperture_auth::LoginResult, ApiError> {
    limiter.check(username)?;
    match auth_call.await {
        Ok(Some(result)) => {
            limiter.record_success(username);
            Ok(result)
        }
        Ok(None) => Err(ApiError::CONFLICT),
        Err(aperture_auth::AuthError::InvalidCredentials) => {
            limiter.record_failure(username);
            Err(ApiError::UNAUTHORIZED)
        }
        Err(err) => Err(err.into()),
    }
}
