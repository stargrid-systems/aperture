use aperture_auth::{Action, AuthenticatedActor, Object, Password, Role, Username};
use aperture_storage::{ActorId, UserId};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::operation_ids;
use crate::AppState;
use crate::avatar::{AvatarAnimation, AvatarStyle};
use crate::conditional::Etag;
use crate::dto::{Page, SimpleListParams};
use crate::error::ApiError;

/// `Cache-Control` for avatar responses. The representation can change at
/// runtime through the avatar settings, so it is not marked `immutable`.
const CACHE_CONTROL_AVATAR: HeaderValue = HeaderValue::from_static("public, max-age=31536000");

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_users, create_user))
        .routes(routes!(get_user))
        .routes(routes!(delete_user))
        .routes(routes!(user_avatar))
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserResponse {
    id: UserId,
    actor_id: ActorId,
    username: String,
    must_change_password: bool,
}

impl From<aperture_storage::User> for UserResponse {
    fn from(user: aperture_storage::User) -> Self {
        Self {
            id: user.id,
            actor_id: user.actor_id,
            username: user.username,
            must_change_password: user.password_change_required_at.is_some(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    username: Username,
    password: Password,
    role: Option<Role>,
}

/// Lists all users.
#[utoipa::path(
    get,
    path = "",
    operation_id = operation_ids::LIST_USERS,
    extensions(("x-required-permission" = json!("user:read"))),
    params(SimpleListParams),
    responses((status = 200, description = "Users", body = Page<UserResponse>)),
)]
async fn list_users(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Query(params): Query<SimpleListParams>,
) -> Result<Json<Page<UserResponse>>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::User, Action::Read)
        .await?;
    let page = state.auth().list_users(&params.to_query()).await?;
    Ok(Json(Page::from_storage(page, UserResponse::from)))
}

/// Creates a new user.
#[utoipa::path(
    post,
    path = "",
    operation_id = operation_ids::CREATE_USER,
    extensions(("x-required-permission" = json!("user:create"))),
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = UserResponse),
        (status = 403, description = "Insufficient permissions"),
    ),
)]
async fn create_user(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Json(request): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::User, Action::Create)
        .await?;
    let actor = state
        .auth()
        .create_user(&request.username, &request.password, None)
        .await?;
    if let Some(role) = &request.role {
        let subject = aperture_auth::actor_subject(actor.id);
        if let Err(err) = state.auth().assign_role(&subject, *role).await {
            let now = jiff::Timestamp::now();
            let _ = state.storage().actors()?.disable(actor.id, now).await;
            return Err(err.into());
        }
    }
    let user = state
        .storage()
        .users()?
        .find_by_actor_id(actor.id)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

/// Returns one user.
#[utoipa::path(
    get,
    path = "/{id}",
    operation_id = operation_ids::GET_USER,
    extensions(("x-required-permission" = json!("user:read"))),
    params(("id" = UserId, Path, description = "User id")),
    responses(
        (status = 200, description = "User", body = UserResponse),
        (status = 404, description = "Unknown user"),
    ),
)]
async fn get_user(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(id): Path<UserId>,
) -> Result<Json<UserResponse>, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::User, Action::Read)
        .await?;
    let user = state
        .storage()
        .users()?
        .get(id)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    Ok(Json(UserResponse::from(user)))
}

/// Deletes a user.
#[utoipa::path(
    delete,
    path = "/{id}",
    operation_id = operation_ids::DELETE_USER,
    extensions(("x-required-permission" = json!("user:delete"))),
    params(("id" = UserId, Path, description = "User id")),
    responses(
        (status = 204, description = "User deleted"),
        (status = 404, description = "Unknown user"),
    ),
)]
async fn delete_user(
    auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(id): Path<UserId>,
) -> Result<StatusCode, ApiError> {
    state
        .auth()
        .require(&auth.subject, Object::User, Action::Delete)
        .await?;
    let user = state
        .storage()
        .users()?
        .get(id)
        .await?
        .ok_or(ApiError::NOT_FOUND)?;
    state
        .auth()
        .delete_user(user.id, user.actor_id, auth.actor.id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Returns a deterministic avatar for a user as inline SVG.
///
/// The avatar style and animation are read from the `avatar_style` and
/// `avatar_animation` settings on every request, so the output can be
/// reconfigured at runtime. The response carries a strong `ETag` derived from
/// the active style and animation, so a client that already has a fresh copy
/// (per `If-None-Match`) gets a `304 Not Modified` with an empty body.
#[utoipa::path(
    get,
    path = "/{id}/avatar",
    operation_id = operation_ids::GET_USER_AVATAR,
    params(("id" = UserId, Path, description = "User id")),
    responses(
        (status = 200, description = "Avatar SVG", content_type = "image/svg+xml",
         headers(
            ("Cache-Control" = String, description = "Caching directive"),
            ("ETag" = String, description = "Strong tag derived from the active style and animation"),
         )),
        (status = 304, description = "Not Modified",
         headers(
            ("ETag" = String, description = "Strong tag derived from the active style and animation"),
            ("Cache-Control" = String, description = "Caching directive"),
         )),
    ),
)]
async fn user_avatar(
    _auth: AuthenticatedActor,
    State(state): State<AppState>,
    Path(id): Path<UserId>,
    headers: HeaderMap,
) -> Result<Response<Body>, ApiError> {
    let style_setting = state.settings().get::<AvatarStyle>().await?;
    let animation_setting = state.settings().get::<AvatarAnimation>().await?;

    let etag = Etag::wrap(
        HeaderValue::from_str(&format!(
            "\"avatar-{}-{}\"",
            style_setting.as_str(),
            animation_setting.as_str()
        ))
        .expect("avatar etag is valid ASCII"),
    );

    if etag.matches_if_none_match(&headers) {
        return Ok(Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, HeaderValue::from(etag))
            .header(header::CACHE_CONTROL, CACHE_CONTROL_AVATAR)
            .body(Body::empty())
            .expect("valid 304 response"));
    }

    let svg = dicebear_lite::Avatar::new(style_setting.style(), &id.to_string())
        .animation(animation_setting.animation())
        .to_string();
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .header(header::CACHE_CONTROL, CACHE_CONTROL_AVATAR)
        .header(header::ETAG, HeaderValue::from(etag))
        .body(Body::from(svg))
        .expect("valid response"))
}
