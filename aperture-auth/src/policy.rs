//! Casbin model definition, the typed permission vocabulary, and built-in
//! policy seeding.
//!
//! Authorization is expressed with the typed [`Object`] and [`Action`] enums so
//! that [`AuthHandle::require`](crate::AuthHandle::require) cannot be called
//! with an arbitrary string. Roles are the closed [`Role`] enum. Adding a new
//! object or action is a deliberate change that also forces a policy-seed
//! update, which keeps the vocabulary and the granted permissions in sync.

use std::collections::HashSet;
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
    TaskSchedule,
    Log,
    User,
    ApiKey,
    Setting,
}

impl Object {
    /// The casbin policy string for this object.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Artifact => "artifact",
            Self::Task => "task",
            Self::TaskSchedule => "task-schedule",
            Self::Log => "log",
            Self::User => "user",
            Self::ApiKey => "api-key",
            Self::Setting => "setting",
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

/// The `OpenAPI` extension name carrying an operation's required permission.
pub const REQUIRED_PERMISSION_EXTENSION: &str = "x-required-permission";

/// The permission string for an authorization check on `object` and
/// `action`, e.g. `"task:read"`.
///
/// The `OpenAPI` annotations reference the enum variants through this
/// function, so a typo or vocabulary rename fails to compile instead of
/// silently corrupting the spec.
pub fn required_permission(object: Object, action: Action) -> String {
    format!("{object}:{action}")
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

/// Adds the built-in role and permission grants that are missing.
///
/// Runs on every boot so databases seeded by older builds receive grants
/// introduced since, and so an interrupted first boot self-heals on the next
/// start. Existing rows are never modified or removed.
///
/// Admin is the superuser (`*:*`). Operator and viewer get explicit per-object
/// grants rather than wildcards, so adding a new object never silently becomes
/// accessible. Notably, a viewer can read artifact catalog metadata but cannot
/// download artifact blobs (which include secrets such as TLS private keys).
pub async fn sync_builtin_policies(e: &mut Enforcer) -> casbin::Result<()> {
    use casbin::{CoreApi, MgmtApi};

    fn policy(role: Role, obj: impl fmt::Display, act: impl fmt::Display) -> Vec<String> {
        vec![role.to_string(), obj.to_string(), act.to_string()]
    }

    let policies = vec![
        // Admin: superuser.
        policy(Role::Admin, "*", "*"),
        // Operator: full operational access, no user or api-key management.
        policy(Role::Operator, Object::Artifact, "*"),
        policy(Role::Operator, Object::Task, "*"),
        policy(Role::Operator, Object::TaskSchedule, "*"),
        policy(Role::Operator, Object::Log, Action::Read),
        policy(Role::Operator, Object::Setting, "*"),
        // Viewer: read-only on non-sensitive data. No artifact downloads.
        policy(Role::Viewer, Object::Artifact, Action::Read),
        policy(Role::Viewer, Object::Task, Action::Read),
        policy(Role::Viewer, Object::TaskSchedule, Action::Read),
        policy(Role::Viewer, Object::Log, Action::Read),
        policy(Role::Viewer, Object::Setting, Action::Read),
    ];
    // Only the rules the model does not hold yet. Casbin's add_policies drops
    // the whole batch when any rule is already present.
    let existing: HashSet<Vec<String>> = e.get_model().get_policy("p", "p").into_iter().collect();
    let missing: Vec<Vec<String>> = policies
        .into_iter()
        .filter(|rule| !existing.contains(rule))
        .collect();
    if !missing.is_empty() {
        e.add_policies(missing).await?;
    }
    Ok(())
}

/// Subject string for a session-authenticated actor.
pub fn actor_subject(actor_id: aperture_storage::ActorId) -> String {
    format!("actor:{}", actor_id.get())
}

/// Subject string for an API key.
pub fn apikey_subject(key_id: aperture_storage::ApiKeyId) -> String {
    format!("apikey:{}", key_id.get())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use aperture_storage::PolicyType;
    use casbin::RbacApi;

    use super::*;

    /// Simulates a restart.
    ///
    /// Seeds one enforcer, drops it, then loads a second enforcer over the
    /// same storage. The persisted rules must keep their real arity so
    /// enforcement and role assignments still work.
    #[tokio::test]
    async fn seeded_policies_survive_reload() {
        let storage = Storage::open(":memory:").await.unwrap();
        {
            let mut e = create_enforcer(&storage).await.unwrap();
            sync_builtin_policies(&mut e).await.unwrap();
            e.add_role_for_user("actor:7", Role::Viewer.as_str(), None)
                .await
                .unwrap();
        }

        let e = create_enforcer(&storage).await.unwrap();

        assert!(
            e.enforce(("actor:7", Object::Artifact.as_str(), Action::Read.as_str()))
                .unwrap()
        );
        assert!(
            !e.enforce((
                "actor:7",
                Object::Artifact.as_str(),
                Action::Download.as_str()
            ))
            .unwrap()
        );
        assert!(e.has_role_for_user("actor:7", Role::Viewer.as_str(), None));
    }

    /// Syncing on every boot must not duplicate rows.
    ///
    /// Two syncs over the same storage leave the row count stable and
    /// enforcement working.
    #[tokio::test]
    async fn repeated_sync_does_not_duplicate_rules() {
        let storage = Storage::open(":memory:").await.unwrap();
        let repo = storage.policy().unwrap();

        {
            let mut e = create_enforcer(&storage).await.unwrap();
            sync_builtin_policies(&mut e).await.unwrap();
        }
        let count_after_first_boot = repo.count().await.unwrap();

        let mut e = create_enforcer(&storage).await.unwrap();
        sync_builtin_policies(&mut e).await.unwrap();
        assert_eq!(repo.count().await.unwrap(), count_after_first_boot);

        e.add_role_for_user("actor:1", Role::Admin.as_str(), None)
            .await
            .unwrap();
        assert!(
            e.enforce(("actor:1", Object::User.as_str(), Action::Delete.as_str()))
                .unwrap()
        );
    }

    /// Backfills grants missing from a partially seeded table.
    ///
    /// A table seeded by an older build (or an interrupted first boot) holds
    /// only part of the built-in grants. The next sync must add the missing
    /// grants so they are enforced right away.
    #[tokio::test]
    async fn sync_backfills_missing_builtin_grants() {
        let storage = Storage::open(":memory:").await.unwrap();
        let repo = storage.policy().unwrap();
        // An older build knew the admin grant and the viewer artifact read,
        // but not the viewer setting read grant.
        repo.insert(
            PolicyType::Policy,
            &["admin".to_owned(), "*".to_owned(), "*".to_owned()],
        )
        .await
        .unwrap();
        repo.insert(
            PolicyType::Policy,
            &[
                "viewer".to_owned(),
                Object::Artifact.as_str().to_owned(),
                Action::Read.as_str().to_owned(),
            ],
        )
        .await
        .unwrap();

        let mut e = create_enforcer(&storage).await.unwrap();
        e.add_role_for_user("actor:5", Role::Viewer.as_str(), None)
            .await
            .unwrap();
        sync_builtin_policies(&mut e).await.unwrap();

        assert!(
            e.enforce(("actor:5", Object::Setting.as_str(), Action::Read.as_str()))
                .unwrap()
        );
        assert!(
            !e.enforce((
                "actor:5",
                Object::Artifact.as_str(),
                Action::Download.as_str()
            ))
            .unwrap()
        );
    }

    /// A rule whose last real token is an empty string must survive a reload
    /// with the empty string and its arity intact.
    ///
    /// Empty used to be indistinguishable from the storage layer's "" padding
    /// and was trimmed on load, silently changing the rule.
    #[tokio::test]
    async fn empty_string_token_survives_reload() {
        let storage = Storage::open(":memory:").await.unwrap();
        let rule = vec![
            "special".to_owned(),
            Object::Task.as_str().to_owned(),
            String::new(),
        ];
        {
            let mut e = create_enforcer(&storage).await.unwrap();
            sync_builtin_policies(&mut e).await.unwrap();
            casbin::MgmtApi::add_policy(&mut e, rule.clone())
                .await
                .unwrap();
        }

        // The stored row keeps the empty string as a real value.
        let stored = storage.policy().unwrap().load_all().await.unwrap();
        let expected = [
            Some("special".to_owned()),
            Some(Object::Task.as_str().to_owned()),
            Some(String::new()),
            None,
            None,
            None,
        ];
        assert!(
            stored
                .iter()
                .any(|rule| rule.values == expected && rule.ptype == PolicyType::Policy)
        );

        let e = create_enforcer(&storage).await.unwrap();
        let policies = e.get_model().get_policy("p", "p");
        assert!(policies.contains(&rule), "rule must reload with arity 3");
    }

    /// The sync only adds p rules.
    ///
    /// Custom g rules (role assignments) must survive it unchanged.
    #[tokio::test]
    async fn sync_leaves_grouping_rules_alone() {
        let storage = Storage::open(":memory:").await.unwrap();
        let repo = storage.policy().unwrap();

        let mut e = create_enforcer(&storage).await.unwrap();
        e.add_role_for_user("actor:3", Role::Viewer.as_str(), None)
            .await
            .unwrap();
        e.add_role_for_user("actor:4", Role::Operator.as_str(), None)
            .await
            .unwrap();
        let before: HashSet<Vec<Option<String>>> = repo
            .load_all()
            .await
            .unwrap()
            .into_iter()
            .filter(|rule| rule.ptype == PolicyType::Grouping)
            .map(|rule| rule.values)
            .collect();

        sync_builtin_policies(&mut e).await.unwrap();

        let after: HashSet<Vec<Option<String>>> = repo
            .load_all()
            .await
            .unwrap()
            .into_iter()
            .filter(|rule| rule.ptype == PolicyType::Grouping)
            .map(|rule| rule.values)
            .collect();
        assert_eq!(before, after);
        assert!(e.has_role_for_user("actor:3", Role::Viewer.as_str(), None));
        assert!(e.has_role_for_user("actor:4", Role::Operator.as_str(), None));
    }
}
