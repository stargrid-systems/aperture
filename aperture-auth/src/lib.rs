//! Authentication and authorization for the Aperture gateway.
//!
//! [`AuthHandle`] is the single entry point. It wraps a casbin enforcer for
//! authorization and the storage layer for credential management.
//!
//! Authentication supports two methods:
//!
//! - **Session cookie**: a browser-friendly flow. `POST /auth/login` returns a
//!   session token stored as an httpOnly cookie.
//! - **API key**: a bearer token for headless clients. Each key has its own
//!   scoped permissions enforced as a separate casbin subject.

use std::result::Result as StdResult;
use std::sync::Arc;
use std::time::Duration;

use aperture_storage::{Actor, ActorId, ActorKind, Storage, UserId};
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use casbin::{CoreApi, Enforcer, MgmtApi, RbacApi};
use jiff::Timestamp;
use tokio::sync::RwLock;

pub use self::error::{AuthError, Result};
pub use self::password::Password;
pub use self::policy::{actor_subject, apikey_subject, roles};
pub use self::token::{RawApiKey, SessionToken};

mod error;
mod password;
mod policy;
mod token;

/// Sliding session lifetime. A session expires after this duration of
/// inactivity.
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The result of a successful login.
#[derive(Debug)]
pub struct LoginResult {
    /// The raw session token. The caller sets it as a cookie.
    pub token: SessionToken,
    /// The authenticated actor.
    pub actor: Actor,
    /// Whether the user must change their password before continuing.
    pub must_change_password: bool,
}

/// An actor resolved from a credential (session or API key).
#[derive(Debug, Clone)]
pub struct AuthenticatedActor {
    /// The resolved actor.
    pub actor: Actor,
    /// The casbin subject string (`"actor:<id>"` or `"apikey:<id>"`).
    pub subject: String,
    /// Whether the user must change their password (session auth only).
    pub must_change_password: bool,
}

/// Axum extractor that reads the actor from request extensions (populated by
/// the auth middleware).
impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedActor {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> StdResult<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthenticatedActor>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

/// The single auth entry point: casbin enforcer plus storage.
#[derive(Clone)]
pub struct AuthHandle {
    storage: Storage,
    enforcer: Arc<RwLock<Enforcer>>,
}

impl AuthHandle {
    /// Creates the auth handle: builds the enforcer, loads existing policies,
    /// and seeds built-in roles if the policy table is empty.
    pub async fn new(storage: Storage) -> Result<Self> {
        let mut enforcer = policy::create_enforcer(&storage)
            .await
            .map_err(AuthError::from_casbin)?;
        policy::seed_builtin_policies(&mut enforcer, &storage)
            .await
            .map_err(AuthError::from_casbin)?;
        Ok(Self {
            storage,
            enforcer: Arc::new(RwLock::new(enforcer)),
        })
    }

    /// Read-only access to the underlying storage.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Checks whether `subject` may perform `action` on `object`.
    pub async fn enforce(&self, subject: &str, obj: &str, act: &str) -> Result<bool> {
        let e = self.enforcer.read().await;
        e.enforce((subject, obj, act))
            .map_err(AuthError::from_casbin)
    }

    /// Assigns `role` to `subject` (e.g. `"actor:1"` -> `"admin"`).
    pub async fn assign_role(&self, subject: &str, role: &str) -> Result<()> {
        let mut e = self.enforcer.write().await;
        e.add_role_for_user(subject, role, None)
            .await
            .map_err(AuthError::from_casbin)?;
        Ok(())
    }

    /// Removes `role` from `subject`.
    pub async fn revoke_role(&self, subject: &str, role: &str) -> Result<()> {
        let mut e = self.enforcer.write().await;
        e.delete_role_for_user(subject, role, None)
            .await
            .map_err(AuthError::from_casbin)?;
        Ok(())
    }

    /// Returns the list of roles assigned to `subject`.
    pub async fn roles_for(&self, subject: &str) -> Result<Vec<String>> {
        let e = self.enforcer.read().await;
        Ok(e.get_roles_for_user(subject, None))
    }

    /// Grants a direct permission to `subject`.
    pub async fn grant_permission(&self, subject: &str, obj: &str, act: &str) -> Result<()> {
        let mut e = self.enforcer.write().await;
        e.add_policy(vec![subject.to_owned(), obj.to_owned(), act.to_owned()])
            .await
            .map_err(AuthError::from_casbin)?;
        Ok(())
    }

    /// Removes all direct permissions for `subject`.
    pub async fn revoke_permissions(&self, subject: &str) -> Result<()> {
        let mut e = self.enforcer.write().await;
        e.remove_filtered_policy(0, vec![subject.to_owned()])
            .await
            .map_err(AuthError::from_casbin)?;
        Ok(())
    }

    /// Verifies `username` / `password` and creates a new session.
    /// Returns the session token for the caller to set as a cookie.
    pub async fn login(&self, username: &str, password: &Password) -> Result<LoginResult> {
        let users = self.storage.users()?;
        let user = users
            .find_by_username(username)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        if !password.verify_against(&user.password_hash)? {
            return Err(AuthError::InvalidCredentials);
        }
        let actors = self.storage.actors()?;
        let actor = actors
            .get(user.actor_id)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        if actor.disabled_at.is_some() {
            return Err(AuthError::ActorDisabled);
        }
        let token = SessionToken::generate();
        let token_hash = token.hash();
        let now = Timestamp::now();
        let expires_at = now + SESSION_TTL;
        let sessions = self.storage.sessions()?;
        sessions
            .create(user.actor_id, &token_hash, expires_at, now)
            .await?;
        Ok(LoginResult {
            token,
            actor,
            must_change_password: user.password_change_required_at.is_some(),
        })
    }

    /// Resolves a session token to an authenticated actor. Extends the
    /// session expiry (sliding window).
    pub async fn resolve_session(
        &self,
        token: &SessionToken,
    ) -> Result<Option<AuthenticatedActor>> {
        let token_hash = token.hash();
        let sessions = self.storage.sessions()?;
        let session = match sessions.find_by_token_hash(&token_hash).await? {
            Some(s) => s,
            None => return Ok(None),
        };
        let now = Timestamp::now();
        if session.expires_at < now {
            return Ok(None);
        }
        let actors = self.storage.actors()?;
        let actor = match actors.get(session.actor_id).await? {
            Some(a) if a.disabled_at.is_none() => a,
            _ => return Ok(None),
        };
        let must_change = if actor.kind == ActorKind::User {
            let users = self.storage.users()?;
            users
                .find_by_actor_id(actor.id)
                .await?
                .map(|u| u.password_change_required_at.is_some())
                .unwrap_or(false)
        } else {
            false
        };
        let new_expiry = now + SESSION_TTL;
        sessions.touch_expiry(session.id, new_expiry).await?;
        Ok(Some(AuthenticatedActor {
            actor,
            subject: actor_subject(session.actor_id),
            must_change_password: must_change,
        }))
    }

    /// Deletes a session (logout).
    pub async fn delete_session(&self, session_token: &SessionToken) -> Result<()> {
        let token_hash = session_token.hash();
        let sessions = self.storage.sessions()?;
        if let Some(session) = sessions.find_by_token_hash(&token_hash).await? {
            sessions.delete(session.id).await?;
        }
        Ok(())
    }

    /// Deletes all expired sessions. Returns how many were removed.
    pub async fn delete_expired_sessions(&self) -> Result<usize> {
        let sessions = self.storage.sessions()?;
        Ok(sessions.delete_expired(Timestamp::now()).await?)
    }

    /// Creates a new API key for `actor_id` with `name`. Returns the raw key
    /// (only visible at creation time). The caller should grant permissions or
    /// assign a role for the new key's subject.
    pub async fn create_api_key(
        &self,
        actor_id: ActorId,
        name: &str,
    ) -> Result<(RawApiKey, aperture_storage::ApiKey)> {
        let raw_key = RawApiKey::generate();
        let prefix = raw_key
            .lookup_prefix()
            .ok_or(AuthError::InvalidCredentials)?;
        let key_hash = raw_key.hash();
        let now = Timestamp::now();
        let repo = self.storage.api_keys()?;
        let api_key = repo.create(actor_id, name, &key_hash, &prefix, now).await?;
        Ok((raw_key, api_key))
    }

    /// Resolves an API key to an authenticated actor.
    pub async fn resolve_api_key(&self, key: &RawApiKey) -> Result<Option<AuthenticatedActor>> {
        let prefix = match key.lookup_prefix() {
            Some(p) => p,
            None => return Ok(None),
        };
        let repo = self.storage.api_keys()?;
        let api_key = match repo.find_by_prefix(&prefix).await? {
            Some(k) => k,
            None => return Ok(None),
        };
        let key_hash = key.hash();
        if !key_hash.matches(&api_key.key_hash) {
            return Ok(None);
        }
        let actors = self.storage.actors()?;
        let actor = match actors.get(api_key.actor_id).await? {
            Some(a) if a.disabled_at.is_none() => a,
            _ => return Ok(None),
        };
        let now = Timestamp::now();
        repo.touch_last_used(api_key.id, now).await?;
        Ok(Some(AuthenticatedActor {
            actor,
            subject: apikey_subject(api_key.id),
            must_change_password: false,
        }))
    }

    /// Creates a new user actor and user record. Returns the actor.
    /// If `password_change_required_at` is `Some`, the user must change their
    /// password before accessing any other endpoint.
    pub async fn create_user(
        &self,
        username: &str,
        password: &Password,
        password_change_required_at: Option<Timestamp>,
    ) -> Result<Actor> {
        let now = Timestamp::now();
        let hash = password.hash()?;
        let actors = self.storage.actors()?;
        let actor = actors.create(ActorKind::User, username, now).await?;
        let users = self.storage.users()?;
        users
            .create(actor.id, username, &hash, password_change_required_at, now)
            .await?;
        Ok(actor)
    }

    /// Changes the password for user `user_id`.
    pub async fn change_password(&self, user_id: UserId, new_password: &Password) -> Result<()> {
        let hash = new_password.hash()?;
        let users = self.storage.users()?;
        users.update_password(user_id, &hash, None).await?;
        Ok(())
    }

    /// Lists all users.
    pub async fn list_users(&self) -> Result<Vec<aperture_storage::User>> {
        let users = self.storage.users()?;
        Ok(users.list().await?)
    }

    /// Deletes user `user_id` and disables the associated actor.
    pub async fn delete_user(&self, user_id: UserId, actor_id: ActorId) -> Result<()> {
        let now = Timestamp::now();
        let users = self.storage.users()?;
        users.delete(user_id).await?;
        let actors = self.storage.actors()?;
        actors.disable(actor_id, now).await?;
        let sessions = self.storage.sessions()?;
        sessions.delete_for_actor(actor_id).await?;
        self.revoke_permissions(&actor_subject(actor_id)).await?;
        self.revoke_role(&actor_subject(actor_id), roles::ADMIN)
            .await?;
        self.revoke_role(&actor_subject(actor_id), roles::OPERATOR)
            .await?;
        self.revoke_role(&actor_subject(actor_id), roles::VIEWER)
            .await?;
        Ok(())
    }

    /// Returns true when no users exist yet (first-run setup needed).
    pub async fn is_setup_required(&self) -> Result<bool> {
        let users = self.storage.users()?;
        Ok(users.count().await? == 0)
    }

    /// Creates the initial admin user and returns a login result with a
    /// session token. Only succeeds when no users exist. The caller is
    /// responsible for checking [`Self::is_setup_required`] first.
    pub async fn setup_admin(&self, username: &str, password: &Password) -> Result<LoginResult> {
        let actor = self.create_user(username, password, None).await?;
        let subject = actor_subject(actor.id);
        self.assign_role(&subject, roles::ADMIN).await?;
        tracing::info!(actor = actor.id.get(), "setup admin user");
        self.login(username, password).await
    }
}
