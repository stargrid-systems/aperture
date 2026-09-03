//! Authentication and authorization for the Aperture gateway.
//!
//! [`AuthHandle`] is the single entry point. It pairs the storage layer for
//! credential management with a plain-Rust policy decision point, [`authz`].
//!
//! Authorization is decided by code. The typed permission vocabulary, the
//! unforgeable [`Permit`], and the role matrix all live in
//! [`authz`]; the database stores only which subject holds which role, so
//! there is nothing to seed or sync on boot.
//!
//! Authentication supports two methods:
//!
//! - **Session cookie**: a browser-friendly flow. `POST /auth/login` returns a
//!   session token stored as an httpOnly cookie.
//! - **API key**: a bearer token for headless clients. Each key acts as its own
//!   [`Subject`] with its own role assignments.

use std::collections::HashMap;
use std::error::Error as StdError;
use std::result::Result as StdResult;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use aperture_storage::{
    Actor, ActorId, ActorKind, ApiKeyId, Storage, SubjectKind, TokenHash, UserId,
};
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use jiff::Timestamp;
use tokio::sync::RwLock;

use self::authz::{Permission, Permit, Role, Subject, role_allows};
pub use self::error::{AuthError, Result};
pub use self::password::Password;
pub use self::ratelimit::LoginLimiter;
pub use self::token::{RawApiKey, SessionToken};
pub use self::username::Username;

pub mod authz;
mod error;
mod password;
mod ratelimit;
mod token;
mod username;

/// Sliding session lifetime. A session expires after this duration of
/// inactivity.
const SESSION_TTL: Duration = Duration::from_hours(168);

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
    /// The authenticated subject (`Subject::Actor` or `Subject::ApiKey`).
    pub subject: Subject,
    /// Whether the user must change their password (session auth only).
    pub must_change_password: bool,
}

/// Axum extractor that reads the actor from request extensions (populated by
/// the auth middleware).
impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedActor {
    type Rejection = StatusCode;

    #[expect(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> StdResult<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

/// The single auth entry point: the PDP plus credential storage.
#[derive(Clone)]
pub struct AuthHandle {
    storage: Storage,
    /// In-memory index of role assignments, keyed by subject. Permission
    /// grants live in `authz::role_allows`; this index only mirrors the
    /// `role_assignment` table.
    roles: Arc<RwLock<HashMap<Subject, Vec<Role>>>>,
}

impl AuthHandle {
    /// Creates the auth handle.
    ///
    /// Rebuilds the in-memory role index from the role assignments in
    /// storage. Permission grants live in code (`authz::role_allows`); the
    /// database stores only role assignments, so there is nothing to seed or
    /// sync on boot.
    ///
    /// # Errors
    ///
    /// Returns an error if role assignments cannot be read or a stored role
    /// string is not a built-in role.
    pub async fn new(storage: Storage) -> Result<Self> {
        let repo = storage.role_assignments()?;
        let mut index: HashMap<Subject, Vec<Role>> = HashMap::new();
        // Iterating roles in `Role::ALL` order builds each subject's list
        // pre-sorted, keeping `/auth/me`-style output deterministic.
        for role in Role::ALL {
            let assignments = repo.subjects_with_role(role.as_str()).await?;
            for assignment in assignments {
                let subject = match assignment.kind {
                    SubjectKind::Actor => Subject::Actor(ActorId::from(assignment.subject_id)),
                    SubjectKind::ApiKey => Subject::ApiKey(ApiKeyId::from(assignment.subject_id)),
                };
                let role = assignment.role.parse::<Role>()?;
                index.entry(subject).or_default().push(role);
            }
        }
        Ok(Self {
            storage,
            roles: Arc::new(RwLock::new(index)),
        })
    }

    /// Requires that `subject` holds a role granting the permission `P`.
    /// Returns a [`Permit`] that is minted only on success.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Forbidden`] if no role of `subject` grants `P`.
    pub async fn require<P: Permission>(&self, subject: &Subject) -> Result<Permit<P>> {
        let index = self.roles.read().await;
        let allowed = index
            .get(subject)
            .is_some_and(|roles| roles.iter().any(|r| role_allows(*r, P::OBJECT, P::ACTION)));
        if allowed {
            Ok(Permit::mint())
        } else {
            Err(AuthError::Forbidden)
        }
    }

    /// Assigns `role` to `subject`. Persists the assignment and updates the
    /// in-memory index. Assigning a role the subject already holds is a
    /// no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if the assignment cannot be persisted.
    pub async fn assign_role(&self, subject: Subject, role: Role) -> Result<()> {
        self.storage
            .role_assignments()?
            .insert(subject.kind(), subject.db_id(), role.as_str())
            .await?;
        let mut index = self.roles.write().await;
        insert_sorted(index.entry(subject).or_default(), role);
        Ok(())
    }

    /// Removes every role assigned to `subject`. Deletes the rows from
    /// storage and drops the subject's entry from the in-memory index.
    ///
    /// # Errors
    ///
    /// Returns an error if the deletion cannot be persisted.
    pub async fn revoke_roles(&self, subject: Subject) -> Result<()> {
        self.storage
            .role_assignments()?
            .delete_for_subject(subject.kind(), subject.db_id())
            .await?;
        let mut index = self.roles.write().await;
        index.remove(&subject);
        Ok(())
    }

    /// Returns the roles assigned to `subject`, ordered by [`Role::ALL`].
    ///
    /// # Errors
    ///
    /// Never returns an error today; the signature is kept uniform with the
    /// other fallible handle methods.
    pub async fn roles_for(&self, subject: &Subject) -> Result<Vec<Role>> {
        let index = self.roles.read().await;
        Ok(index.get(subject).cloned().unwrap_or_default())
    }

    /// Verifies `username` / `password` and creates a new session.
    /// Returns the session token for the caller to set as a cookie.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid credentials, a disabled actor, or storage
    /// failures.
    pub async fn login(&self, username: &Username, password: &Password) -> Result<LoginResult> {
        let users = self.storage.users()?;
        let Some(user) = users.find_by_username(username.as_str()).await? else {
            let _ = password.verify_against(&DUMMY_HASH);
            return Err(AuthError::InvalidCredentials);
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
    /// # Errors
    ///
    /// Returns an error if the storage layer fails during session or actor
    /// lookup, or password verification fails.
    pub async fn resolve_session(
        &self,
        token: &SessionToken,
    ) -> Result<Option<AuthenticatedActor>> {
        let token_hash = token.hash();
        let sessions = self.storage.sessions()?;
        let Some(session) = sessions.find_by_token_hash(&token_hash).await? else {
            return Ok(None);
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
                .is_some_and(|u| u.password_change_required_at.is_some())
        } else {
            false
        };
        let new_expiry = now + SESSION_TTL;
        if session.expires_at <= now + SESSION_TTL / 2 {
            sessions.touch_expiry(session.id, new_expiry).await?;
        }
        Ok(Some(AuthenticatedActor {
            actor,
            subject: Subject::Actor(session.actor_id),
            must_change_password: must_change,
        }))
    }

    /// Deletes a session (logout).
    ///
    /// # Errors
    ///
    /// Returns an error if the storage layer fails to find or delete the
    /// session.
    pub async fn delete_session(&self, session_token: &SessionToken) -> Result<()> {
        let token_hash = session_token.hash();
        let sessions = self.storage.sessions()?;
        if let Some(session) = sessions.find_by_token_hash(&token_hash).await? {
            sessions.delete(session.id).await?;
        }
        Ok(())
    }

    /// Deletes all expired sessions. Returns how many were removed.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage layer fails to delete sessions.
    pub async fn delete_expired_sessions(&self) -> Result<usize> {
        let sessions = self.storage.sessions()?;
        Ok(sessions.delete_expired(Timestamp::now()).await?)
    }

    /// Creates a new API key for `actor_id` with `name`. Returns the raw key
    /// (only visible at creation time). The caller should assign a role to
    /// the new key's subject (`Subject::ApiKey(api_key.id)`) so the key can
    /// authorize requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the key prefix is invalid or storage fails to
    /// persist the key.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the storage layer fails to look up the key or
    /// actor.
    pub async fn resolve_api_key(&self, key: &RawApiKey) -> Result<Option<AuthenticatedActor>> {
        let Some(prefix) = key.lookup_prefix() else {
            return Ok(None);
        };
        let repo = self.storage.api_keys()?;
        let Some(api_key) = repo.find_by_prefix(&prefix).await? else {
            return Ok(None);
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
            subject: Subject::ApiKey(api_key.id),
            must_change_password: false,
        }))
    }

    /// Creates a new user actor and user record. Returns the actor.
    /// If `password_change_required_at` is `Some`, the user must change their
    /// password before accessing any other endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the password fails validation, hashing fails, or
    /// storage fails.
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
    ///
    /// # Errors
    ///
    /// Returns an error for invalid current password, password reuse,
    /// validation failure, or storage errors.
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

    /// Lists users, paginated.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage layer fails to list users.
    pub async fn list_users(
        &self,
        query: &aperture_storage::ListQuery,
    ) -> Result<aperture_storage::Page<aperture_storage::User>> {
        let users = self.storage.users()?;
        Ok(users.list(query).await?)
    }

    /// Deletes user `user_id` and disables the associated actor.
    ///
    /// Rejects self-deletion. The actor is disabled before its roles are
    /// removed, so any failure during the later steps leaves a
    /// disabled-but-not-purged actor, which is safe: `resolve_session` and
    /// `resolve_api_key` both reject disabled actors.
    ///
    /// API keys are not explicitly revoked: the disabled actor cannot
    /// authenticate, so existing API keys become inert.
    ///
    /// The last-admin check is defense-in-depth for future permission model
    /// changes. Under the current model only admins hold `user:delete` and
    /// the caller always counts as "other", so the check is unreachable in
    /// practice.
    ///
    /// # Errors
    ///
    /// Returns an error for self-deletion, last-admin removal, or storage
    /// failures.
    pub async fn delete_user(
        &self,
        user_id: UserId,
        actor_id: ActorId,
        caller_actor_id: ActorId,
    ) -> Result<()> {
        if actor_id == caller_actor_id {
            return Err(AuthError::CannotDeleteSelf);
        }

        // Check last-admin and mutate under one write lock. The check and the
        // removal must be atomic: with separate locks, two admins deleting
        // each other concurrently could both pass the check and leave the
        // system with zero admins, which re-arms public setup.
        // The self-delete guard above ensures the check can never see zero
        // other admins in practice (the caller is always an admin and always
        // counts as "other"). This is defense-in-depth for future permission
        // model changes.
        let target = Subject::Actor(actor_id);
        {
            let mut index = self.roles.write().await;
            let other_admins = index
                .iter()
                .filter(|(subject, _)| **subject != target)
                .filter(|(_, roles)| roles.contains(&Role::Admin))
                .count();
            if other_admins == 0 {
                return Err(AuthError::LastAdmin);
            }

            // Disable the actor first. If subsequent steps fail, a disabled
            // actor with stale roles is safe: resolve_session and
            // resolve_api_key both reject disabled actors.
            let now = Timestamp::now();
            let actors = self.storage.actors()?;
            actors.disable(actor_id, now).await?;

            // Remove all role assignments from storage and the index.
            self.storage
                .role_assignments()?
                .delete_for_subject(target.kind(), target.db_id())
                .await?;
            index.remove(&target);
        }

        // Clean up remaining storage state.
        let users = self.storage.users()?;
        users.delete(user_id).await?;
        let sessions = self.storage.sessions()?;
        sessions.delete_for_actor(actor_id).await?;
        Ok(())
    }

    /// Returns true when no subject holds the admin role (first-run setup
    /// needed).
    ///
    /// # Errors
    ///
    /// Never returns an error today; the signature is kept uniform with the
    /// other fallible handle methods.
    pub async fn is_setup_required(&self) -> Result<bool> {
        let index = self.roles.read().await;
        Ok(!index.values().any(|roles| roles.contains(&Role::Admin)))
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
    ///
    /// # Errors
    ///
    /// Returns an error for invalid credentials, password validation failure,
    /// or storage failures.
    pub async fn setup_admin(
        &self,
        username: &Username,
        password: &Password,
    ) -> Result<Option<LoginResult>> {
        password.validate()?;
        let now = Timestamp::now();
        let hash = password.hash()?;
        let actor_id = if let Some((actor, _)) = self
            .storage
            .create_initial_user(username.as_str(), &hash, now)
            .await?
        {
            actor.id
        } else {
            // Recovery path. Take a write lock up front so the has_admin
            // check and the role assignment below are atomic: two
            // concurrent recovery calls cannot both see has_admin == false
            // and both assign admin.
            let mut index = self.roles.write().await;
            let has_admin = index.values().any(|roles| roles.contains(&Role::Admin));
            if has_admin {
                return Ok(None);
            }
            let users = self.storage.users()?;
            let Some(user) = users.find_by_username(username.as_str()).await? else {
                // dummy verify to avoid timing oracle (finding L-SEC1)
                let _ = password.verify_against(&DUMMY_HASH);
                return Err(AuthError::InvalidCredentials);
            };
            // Do not grant admin until the caller proves they know the
            // existing password. Without this, anyone who guesses the
            // username during the interrupted-setup window could promote
            // the account.
            if !password.verify_against(&user.password_hash)? {
                return Err(AuthError::InvalidCredentials);
            }
            // Assign admin while still holding the write lock.
            let subject = Subject::Actor(user.actor_id);
            self.storage
                .role_assignments()?
                .insert(subject.kind(), subject.db_id(), Role::Admin.as_str())
                .await?;
            insert_sorted(index.entry(subject).or_default(), Role::Admin);
            // Return early: role is assigned, now just need to log in.
            tracing::info!(actor = user.actor_id.get(), "setup admin user (recovery)");
            let login = self.login(username, password).await?;
            return Ok(Some(login));
        };
        let subject = Subject::Actor(actor_id);
        self.assign_role(subject, Role::Admin).await?;
        tracing::info!(actor = actor_id.get(), "setup admin user");
        let login = self.login(username, password).await?;
        Ok(Some(login))
    }
}

/// Inserts `role` into `roles` unless already present, keeping the list
/// sorted by [`Role::ALL`] order so role output is deterministic.
fn insert_sorted(roles: &mut Vec<Role>, role: Role) {
    if let Err(pos) = roles.binary_search_by_key(&role_rank(role), |r| role_rank(*r)) {
        roles.insert(pos, role);
    }
}

/// Position of `role` in [`Role::ALL`].
fn role_rank(role: Role) -> usize {
    Role::ALL
        .iter()
        .position(|r| *r == role)
        .expect("every built-in role is in Role::ALL")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "correct-horse-battery";

    fn credentials(username: &str) -> (Username, Password) {
        (
            Username::try_from(username.to_owned()).unwrap(),
            Password::new(PASSWORD.to_owned()),
        )
    }

    async fn new_user(auth: &AuthHandle, username: &str) -> Subject {
        let (username, password) = credentials(username);
        let actor = auth.create_user(&username, &password, None).await.unwrap();
        Subject::Actor(actor.id)
    }

    async fn new_handle() -> (Storage, AuthHandle) {
        let storage = Storage::open(":memory:").await.unwrap();
        let auth = AuthHandle::new(storage.clone()).await.unwrap();
        (storage, auth)
    }

    #[tokio::test]
    async fn setup_flow_assigns_admin_once() {
        let (_storage, auth) = new_handle().await;
        assert!(auth.is_setup_required().await.unwrap());

        let (username, password) = credentials("admin");
        let login = auth
            .setup_admin(&username, &password)
            .await
            .unwrap()
            .unwrap();
        assert!(!auth.is_setup_required().await.unwrap());

        let subject = Subject::Actor(login.actor.id);
        assert_eq!(auth.roles_for(&subject).await.unwrap(), vec![Role::Admin]);

        // Setup is already complete, so the second attempt does nothing.
        assert!(
            auth.setup_admin(&username, &password)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn assign_role_twice_yields_one_entry() {
        let (storage, auth) = new_handle().await;
        let subject = new_user(&auth, "bob").await;

        auth.assign_role(subject, Role::Viewer).await.unwrap();
        auth.assign_role(subject, Role::Viewer).await.unwrap();

        assert_eq!(auth.roles_for(&subject).await.unwrap(), vec![Role::Viewer]);
        let stored = storage
            .role_assignments()
            .unwrap()
            .roles_for(subject.kind(), subject.db_id())
            .await
            .unwrap();
        assert_eq!(stored, vec!["viewer".to_owned()]);
    }

    #[tokio::test]
    async fn revoke_roles_clears_index_and_storage() {
        let (storage, auth) = new_handle().await;
        let subject = new_user(&auth, "bob").await;

        auth.assign_role(subject, Role::Viewer).await.unwrap();
        auth.assign_role(subject, Role::Operator).await.unwrap();
        auth.revoke_roles(subject).await.unwrap();

        assert!(auth.roles_for(&subject).await.unwrap().is_empty());
        let stored = storage
            .role_assignments()
            .unwrap()
            .roles_for(subject.kind(), subject.db_id())
            .await
            .unwrap();
        assert!(stored.is_empty());
    }

    #[tokio::test]
    async fn require_follows_the_role_matrix() {
        let (_storage, auth) = new_handle().await;

        // Admin: allowed user deletion.
        let (username, password) = credentials("admin");
        let login = auth
            .setup_admin(&username, &password)
            .await
            .unwrap()
            .unwrap();
        let admin = Subject::Actor(login.actor.id);
        let _ = auth.require::<authz::user::Delete>(&admin).await.unwrap();

        // Viewer: read allowed, download denied.
        let viewer = new_user(&auth, "viewer").await;
        auth.assign_role(viewer, Role::Viewer).await.unwrap();
        let _ = auth
            .require::<authz::artifact::Read>(&viewer)
            .await
            .unwrap();
        assert!(matches!(
            auth.require::<authz::artifact::Download>(&viewer).await,
            Err(AuthError::Forbidden)
        ));

        // Role-less subject: denied everything.
        let nobody = new_user(&auth, "nobody").await;
        assert!(matches!(
            auth.require::<authz::artifact::Read>(&nobody).await,
            Err(AuthError::Forbidden)
        ));
        assert!(matches!(
            auth.require::<authz::setting::Update>(&nobody).await,
            Err(AuthError::Forbidden)
        ));
    }

    /// A fresh handle rebuilds its index from storage, so roles and
    /// enforcement survive a restart.
    #[tokio::test]
    async fn roles_and_enforcement_survive_restart() {
        let storage = Storage::open(":memory:").await.unwrap();
        let operator = {
            let auth = AuthHandle::new(storage.clone()).await.unwrap();
            let (username, password) = credentials("admin");
            auth.setup_admin(&username, &password)
                .await
                .unwrap()
                .unwrap();
            let subject = new_user(&auth, "ops").await;
            auth.assign_role(subject, Role::Operator).await.unwrap();
            subject
        };

        let auth = AuthHandle::new(storage.clone()).await.unwrap();
        assert!(!auth.is_setup_required().await.unwrap());
        assert_eq!(
            auth.roles_for(&operator).await.unwrap(),
            vec![Role::Operator]
        );
        let _ = auth
            .require::<authz::task::Create>(&operator)
            .await
            .unwrap();
        assert!(matches!(
            auth.require::<authz::user::Delete>(&operator).await,
            Err(AuthError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn delete_user_removes_roles_and_rejects_self_deletion() {
        let (storage, auth) = new_handle().await;
        let (username, password) = credentials("admin");
        let login = auth
            .setup_admin(&username, &password)
            .await
            .unwrap()
            .unwrap();
        let admin = Subject::Actor(login.actor.id);

        let victim = new_user(&auth, "victim").await;
        auth.assign_role(victim, Role::Operator).await.unwrap();
        let victim_actor = ActorId::from(victim.db_id());
        let victim_user = storage
            .users()
            .unwrap()
            .find_by_actor_id(victim_actor)
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(
            auth.delete_user(victim_user.id, victim_actor, victim_actor)
                .await,
            Err(AuthError::CannotDeleteSelf)
        ));

        auth.delete_user(victim_user.id, victim_actor, ActorId::from(admin.db_id()))
            .await
            .unwrap();

        assert!(auth.roles_for(&victim).await.unwrap().is_empty());
        let stored = storage
            .role_assignments()
            .unwrap()
            .roles_for(victim.kind(), victim.db_id())
            .await
            .unwrap();
        assert!(stored.is_empty());
    }
}
