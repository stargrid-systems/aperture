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
//!
//! Authorization uses the typed [`Object`] and [`Action`] enums so a check can
//! never reference a nonexistent resource or operation.

use std::error::Error as StdError;
use std::result::Result as StdResult;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use aperture_storage::{Actor, ActorId, ActorKind, Storage, TokenHash, UserId};
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use casbin::{CoreApi, Enforcer, MgmtApi, RbacApi};
use jiff::Timestamp;
use tokio::sync::RwLock;

pub use self::error::{AuthError, Result};
pub use self::password::Password;
pub use self::policy::{Action, Object, Role, actor_subject, apikey_subject};
pub use self::ratelimit::LoginLimiter;
pub use self::token::{RawApiKey, SessionToken};
pub use self::username::Username;

mod error;
mod password;
mod policy;
mod ratelimit;
mod token;
mod username;

/// Sliding session lifetime. A session expires after this duration of
/// inactivity.
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A dummy hash used to keep login timing constant when the username does not
/// exist. The password it hashes is irrelevant; verification always fails.
static DUMMY_HASH: LazyLock<aperture_storage::PasswordHash> = LazyLock::new(|| {
    Password::new("dummy".to_owned())
        .hash()
        .expect("dummy password hash must not fail")
});

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

    /// Requires that `subject` may perform `act` on `obj`. Returns
    /// [`AuthError::Forbidden`] if denied.
    pub async fn require(&self, subject: &str, obj: Object, act: Action) -> Result<()> {
        let e = self.enforcer.read().await;
        if e.enforce((subject, obj.as_str(), act.as_str()))
            .map_err(AuthError::from_casbin)?
        {
            Ok(())
        } else {
            Err(AuthError::Forbidden)
        }
    }

    /// Assigns `role` to `subject` (e.g. `"actor:1"` -> [`Role::Admin`]).
    pub async fn assign_role(&self, subject: &str, role: Role) -> Result<()> {
        let mut e = self.enforcer.write().await;
        e.add_role_for_user(subject, role.as_str(), None)
            .await
            .map_err(AuthError::from_casbin)?;
        Ok(())
    }

    /// Returns the list of roles assigned to `subject`.
    pub async fn roles_for(&self, subject: &str) -> Result<Vec<String>> {
        let e = self.enforcer.read().await;
        Ok(e.get_roles_for_user(subject, None))
    }

    /// Removes all direct permissions for `subject`.
    pub async fn revoke_permissions(&self, subject: &str) -> Result<()> {
        let mut e = self.enforcer.write().await;
        e.remove_filtered_policy(0, vec![subject.to_owned()])
            .await
            .map_err(AuthError::from_casbin)?;
        Ok(())
    }

    /// Returns the casbin subjects currently holding `role`.
    async fn subjects_for_role(&self, role: Role) -> Result<Vec<String>> {
        let e = self.enforcer.read().await;
        Ok(e.get_users_for_role(role.as_str(), None))
    }

    /// Verifies `username` / `password` and creates a new session.
    /// Returns the session token for the caller to set as a cookie.
    pub async fn login(&self, username: &Username, password: &Password) -> Result<LoginResult> {
        let users = self.storage.users()?;
        let user = match users.find_by_username(username.as_str()).await? {
            Some(u) => u,
            None => {
                let _ = password.verify_against(&DUMMY_HASH);
                return Err(AuthError::InvalidCredentials);
            }
        };
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
    // TODO(#153): optionally bind sessions to client attributes so a stolen
    // cookie is harder to reuse from a different client.
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
        if session.expires_at <= now + SESSION_TTL / 2 {
            sessions.touch_expiry(session.id, new_expiry).await?;
        }
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
        repo.touch_last_used_if_stale(api_key.id, now).await?;
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
        username: &Username,
        password: &Password,
        password_change_required_at: Option<Timestamp>,
    ) -> Result<Actor> {
        password.validate()?;
        let now = Timestamp::now();
        let hash = password.hash()?;
        let (actor, _user) = self
            .storage
            .create_user(username.as_str(), &hash, password_change_required_at, now)
            .await?;
        Ok(actor)
    }

    /// Changes the password for the actor `actor_id` after verifying the
    /// caller knows `current_password`.
    ///
    /// The new password must differ from the current one. All of the actor's
    /// sessions are revoked except the one matching `keep_session_hash` (the
    /// caller's current session, if any), so a stolen cookie stops working
    /// immediately while the caller stays logged in. Session revocation is
    /// best-effort: a failure is logged but does not roll back the password
    /// change.
    pub async fn change_password(
        &self,
        actor_id: ActorId,
        current_password: &Password,
        new_password: &Password,
        keep_session_hash: Option<&TokenHash>,
    ) -> Result<()> {
        new_password.validate()?;
        let users = self.storage.users()?;
        let user = users
            .find_by_actor_id(actor_id)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        if !current_password.verify_against(&user.password_hash)? {
            return Err(AuthError::InvalidCredentials);
        }
        if new_password.verify_against(&user.password_hash)? {
            return Err(AuthError::PasswordReuse);
        }
        let hash = new_password.hash()?;
        users.update_password(user.id, &hash, None).await?;
        let sessions = self.storage.sessions()?;
        if let Err(err) = sessions
            .delete_for_actor_except(actor_id, keep_session_hash)
            .await
        {
            tracing::warn!(
                error = &err as &dyn StdError,
                "failed to revoke sessions after password change"
            );
        }
        Ok(())
    }

    /// Lists all users.
    pub async fn list_users(&self) -> Result<Vec<aperture_storage::User>> {
        let users = self.storage.users()?;
        Ok(users.list().await?)
    }

    /// Deletes user `user_id` and disables the associated actor.
    ///
    /// Rejects self-deletion. The actor is disabled before roles are revoked,
    /// so any failure during the later steps leaves a disabled-but-not-purged
    /// actor, which is safe: `resolve_session` and `resolve_api_key` both
    /// reject disabled actors.
    ///
    /// API keys are not explicitly revoked: the disabled actor cannot
    /// authenticate, so existing API keys become inert.
    ///
    /// The last-admin check is defense-in-depth for future permission model
    /// changes. Under the current model only admins hold `User:Delete` and the
    /// caller always counts as "other", so the check is unreachable in
    /// practice.
    pub async fn delete_user(
        &self,
        user_id: UserId,
        actor_id: ActorId,
        caller_actor_id: ActorId,
    ) -> Result<()> {
        if actor_id == caller_actor_id {
            return Err(AuthError::CannotDeleteSelf);
        }

        // Check last-admin before any mutation. The self-delete guard above
        // ensures this check can never see zero other admins in practice
        // (the caller is always an admin and always counts as "other").
        // This is defense-in-depth for future permission model changes.
        let target_subject = actor_subject(actor_id);
        {
            let e = self.enforcer.read().await;
            let other_admins = e
                .get_users_for_role(Role::Admin.as_str(), None)
                .into_iter()
                .filter(|s| s != &target_subject)
                .count();
            if other_admins == 0 {
                return Err(AuthError::LastAdmin);
            }
        }

        // Disable the actor first. If subsequent steps fail, a disabled actor
        // with stale roles is safe: resolve_session and resolve_api_key both
        // reject disabled actors.
        let now = Timestamp::now();
        let actors = self.storage.actors()?;
        actors.disable(actor_id, now).await?;

        // Now revoke all roles and direct policies under a write lock.
        {
            let mut e = self.enforcer.write().await;
            e.remove_filtered_policy(0, vec![target_subject.clone()])
                .await
                .map_err(AuthError::from_casbin)?;
            for role in Role::ALL {
                e.delete_role_for_user(&target_subject, role.as_str(), None)
                    .await
                    .map_err(AuthError::from_casbin)?;
            }
        }

        // Clean up remaining storage state.
        let users = self.storage.users()?;
        users.delete(user_id).await?;
        let sessions = self.storage.sessions()?;
        sessions.delete_for_actor(actor_id).await?;
        Ok(())
    }

    /// Returns true when no admin role is assigned (first-run setup needed).
    pub async fn is_setup_required(&self) -> Result<bool> {
        Ok(self.subjects_for_role(Role::Admin).await?.is_empty())
    }

    /// Creates the initial admin user when no admin role is assigned. The
    /// check and inserts run as one atomic storage transaction, so concurrent
    /// setup attempts cannot both succeed. Returns `None` if setup is already
    /// complete (an admin role is assigned).
    ///
    /// If a previous setup was interrupted after user creation but before role
    /// assignment, the recovery path re-assigns the admin role to the existing
    /// user. The password is never overwritten, and the caller must prove
    /// knowledge of the existing password before the role is granted, so only
    /// the rightful password holder can complete a resumed setup.
    pub async fn setup_admin(
        &self,
        username: &Username,
        password: &Password,
    ) -> Result<Option<LoginResult>> {
        password.validate()?;
        let now = Timestamp::now();
        let hash = password.hash()?;
        let actor_id = match self
            .storage
            .create_initial_user(username.as_str(), &hash, now)
            .await?
        {
            Some((actor, _)) => actor.id,
            None => {
                // Recovery path. Take a write lock up front so the has_admin
                // check and the role assignment below are atomic: two
                // concurrent recovery calls cannot both see has_admin == false
                // and both assign admin.
                let mut e = self.enforcer.write().await;
                let has_admin = !e.get_users_for_role(Role::Admin.as_str(), None).is_empty();
                if has_admin {
                    return Ok(None);
                }
                let users = self.storage.users()?;
                let user = match users.find_by_username(username.as_str()).await? {
                    Some(u) => u,
                    None => {
                        // dummy verify to avoid timing oracle (finding L-SEC1)
                        let _ = password.verify_against(&DUMMY_HASH);
                        return Err(AuthError::InvalidCredentials);
                    }
                };
                // Do not grant admin until the caller proves they know the
                // existing password. Without this, anyone who guesses the
                // username during the interrupted-setup window could promote
                // the account.
                if !password.verify_against(&user.password_hash)? {
                    return Err(AuthError::InvalidCredentials);
                }
                // Assign admin role while still holding the write lock.
                e.add_role_for_user(&actor_subject(user.actor_id), Role::Admin.as_str(), None)
                    .await
                    .map_err(AuthError::from_casbin)?;
                // Return early: role is assigned, now just need to log in.
                tracing::info!(actor = user.actor_id.get(), "setup admin user (recovery)");
                let login = self.login(username, password).await?;
                return Ok(Some(login));
            }
        };
        let subject = actor_subject(actor_id);
        self.assign_role(&subject, Role::Admin).await?;
        tracing::info!(actor = actor_id.get(), "setup admin user");
        let login = self.login(username, password).await?;
        Ok(Some(login))
    }
}
