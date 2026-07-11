//! Casbin model definition and built-in policy seeding.

use ::casbin::{CoreApi, DefaultModel, Enforcer};

use aperture_storage::Storage;

use self::adapter::TursoAdapter;

mod adapter;

/// RBAC model with glob-matching on objects and actions.
const MODEL_TEXT: &str = r#"
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
"#;

/// Built-in role names.
pub mod roles {
    pub const ADMIN: &str = "admin";
    pub const OPERATOR: &str = "operator";
    pub const VIEWER: &str = "viewer";
}

/// Creates and returns the enforcer with the turso adapter, loading existing
/// policies from the database.
pub(crate) async fn create_enforcer(storage: &Storage) -> casbin::Result<Enforcer> {
    let model = DefaultModel::from_str(MODEL_TEXT).await?;
    let adapter = TursoAdapter::new(storage.clone());
    let mut e = Enforcer::new(model, adapter).await?;
    e.enable_auto_save(true);
    Ok(e)
}

/// Seeds the built-in roles and permissions if the policy table is empty.
pub(crate) async fn seed_builtin_policies(
    e: &mut Enforcer,
    storage: &Storage,
) -> casbin::Result<bool> {
    let count = storage
        .casbin()
        .map_err(|err| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(err))))?
        .count()
        .await
        .map_err(|err| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(err))))?;
    if count > 0 {
        return Ok(false);
    }

    use ::casbin::MgmtApi;

    // Admin: all permissions.
    e.add_policy(vec![
        roles::ADMIN.to_owned(),
        "*".to_owned(),
        "*".to_owned(),
    ])
    .await?;

    // Operator: artifact, task, task-definition, log.
    for obj in ["artifact", "task", "task-definition", "log"] {
        e.add_policy(vec![
            roles::OPERATOR.to_owned(),
            obj.to_owned(),
            "*".to_owned(),
        ])
        .await?;
    }

    // Viewer: read-only on everything.
    e.add_policy(vec![
        roles::VIEWER.to_owned(),
        "*".to_owned(),
        "read".to_owned(),
    ])
    .await?;

    Ok(true)
}

/// Subject string for a session-authenticated actor.
pub fn actor_subject(actor_id: aperture_storage::DbId) -> String {
    format!("actor:{}", actor_id.get())
}

/// Subject string for an API key.
pub fn apikey_subject(key_id: aperture_storage::DbId) -> String {
    format!("apikey:{}", key_id.get())
}
