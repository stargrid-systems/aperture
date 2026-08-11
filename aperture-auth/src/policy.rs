//! Casbin model definition, the typed permission vocabulary, and built-in
//! policy seeding.
//!
//! Authorization is expressed with the typed [`Object`] and [`Action`] enums so
//! that [`AuthHandle::require`](crate::AuthHandle::require) cannot be called
//! with an arbitrary string. Roles are the closed [`Role`] enum. Adding a new
//! object or action is a deliberate change that also forces a policy-seed
//! update, which keeps the vocabulary and the granted permissions in sync.

use std::fmt;
use std::str::FromStr;

use aperture_storage::Storage;
use casbin::{CoreApi, DefaultModel, Enforcer};
use serde::{Deserialize, Serialize};

use self::adapter::{TursoAdapter, map_storage_err};
use crate::error::AuthError;

mod adapter;

/// RBAC model with glob-matching on objects and actions.
const MODEL_TEXT: &str = r"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = g(r.sub, p.sub) && globMatch(r.obj, p.obj) && globMatch(r.act, p.act)
";

/// The protected resource a permission targets. A closed vocabulary so every
/// authorization check names a real resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Object {
    Artifact,
    Task,
    TaskDefinition,
    TaskSchedule,
    Log,
    User,
    ApiKey,
    Setting,
    Event,
}

impl Object {
    /// The casbin policy string for this object.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Artifact => "artifact",
            Self::Task => "task",
            Self::TaskDefinition => "task-definition",
            Self::TaskSchedule => "task-schedule",
            Self::Log => "log",
            Self::User => "user",
            Self::ApiKey => "api-key",
            Self::Setting => "setting",
            Self::Event => "event",
        }
    }
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The operation a permission allows on an [`Object`]. A closed vocabulary so
/// every authorization check names a real action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Download,
    Write,
    Evict,
    Create,
    Update,
    Delete,
    Cancel,
}

impl Action {
    /// The casbin policy string for this action.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Download => "download",
            Self::Write => "write",
            Self::Evict => "evict",
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Cancel => "cancel",
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A built-in role. Stored as its lowercase name in casbin grouping rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

impl Role {
    /// The casbin policy string for this role.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
        }
    }

    /// Every built-in role, in display order.
    pub const ALL: [Self; 3] = [Self::Admin, Self::Operator, Self::Viewer];
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = AuthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Self::Admin),
            "operator" => Ok(Self::Operator),
            "viewer" => Ok(Self::Viewer),
            _ => Err(AuthError::UnknownRole(s.to_owned())),
        }
    }
}

/// Creates and returns the enforcer with the turso adapter, loading existing
/// policies from the database.
pub async fn create_enforcer(storage: &Storage) -> casbin::Result<Enforcer> {
    let model = DefaultModel::from_str(MODEL_TEXT).await?;
    let repo = storage.policy().map_err(map_storage_err)?;
    let adapter = TursoAdapter::new(repo);
    let mut e = Enforcer::new(model, adapter).await?;
    e.enable_auto_save(true);
    Ok(e)
}

/// Seeds the built-in roles and permissions if the policy table is empty.
///
/// Admin is the superuser (`*:*`). Operator and viewer get explicit per-object
/// grants rather than wildcards, so adding a new object never silently becomes
/// accessible. Notably, a viewer can read artifact catalog metadata but cannot
/// download artifact blobs (which include secrets such as TLS private keys).
pub async fn seed_builtin_policies(e: &mut Enforcer, storage: &Storage) -> casbin::Result<bool> {
    use casbin::MgmtApi;

    fn policy(role: Role, obj: impl fmt::Display, act: impl fmt::Display) -> Vec<String> {
        vec![role.to_string(), obj.to_string(), act.to_string()]
    }

    let count = storage
        .policy()
        .map_err(map_storage_err)?
        .count()
        .await
        .map_err(map_storage_err)?;
    if count > 0 {
        return Ok(false);
    }

    let policies = vec![
        // Admin: superuser.
        policy(Role::Admin, "*", "*"),
        // Operator: full operational access, no user or api-key management.
        policy(Role::Operator, Object::Artifact, "*"),
        policy(Role::Operator, Object::Task, "*"),
        policy(Role::Operator, Object::TaskDefinition, Action::Read),
        policy(Role::Operator, Object::TaskSchedule, "*"),
        policy(Role::Operator, Object::Log, Action::Read),
        policy(Role::Operator, Object::Setting, "*"),
        policy(Role::Operator, Object::Event, Action::Read),
        // Viewer: read-only on non-sensitive data. No artifact downloads.
        policy(Role::Viewer, Object::Artifact, Action::Read),
        policy(Role::Viewer, Object::Task, Action::Read),
        policy(Role::Viewer, Object::TaskDefinition, Action::Read),
        policy(Role::Viewer, Object::TaskSchedule, Action::Read),
        policy(Role::Viewer, Object::Log, Action::Read),
        policy(Role::Viewer, Object::Setting, Action::Read),
        policy(Role::Viewer, Object::Event, Action::Read),
    ];
    e.add_policies(policies).await?;

    Ok(true)
}

/// Subject string for a session-authenticated actor.
pub fn actor_subject(actor_id: aperture_storage::ActorId) -> String {
    format!("actor:{}", actor_id.get())
}

/// Subject string for an API key.
pub fn apikey_subject(key_id: aperture_storage::ApiKeyId) -> String {
    format!("apikey:{}", key_id.get())
}
