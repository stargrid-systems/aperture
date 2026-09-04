//! The policy decision point: the permission vocabulary, the role matrix,
//! and the unforgeable [`Permit`].
//!
//! Permission grants live in code, not in the database. [`role_allows`] holds
//! the full role matrix, and the database stores only which [`Subject`] holds
//! which [`Role`]. A handler that needs authorization demands a
//! [`Permit`] minted by [`AuthHandle::require`](crate::AuthHandle::require):
//!
//! ```
//! use aperture_auth::authz::{self, Permit};
//!
//! /// Deletes a user. Callable only with the `user:delete` permission.
//! async fn delete_user(permit: Permit<authz::user::Delete>, user_id: u64) {
//! #     let _ = (permit, user_id);
//! }
//! ```
//!
//! `Permit` has no public constructor, so the only way to obtain one is a
//! positive PDP decision. Code demanding a `Permit` argument therefore cannot
//! run without authorization.

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use aperture_storage::{ActorId, ApiKeyId, SubjectKind};
use serde::{Deserialize, Serialize};

use crate::error::AuthError;

/// Defines permission marker types: one unit struct per `(object, action)`
/// pair, each implementing [`Permission`].
///
/// The string literals are passed separately because [`concat!`] only accepts
/// literals and [`Object::as_str`] is not one. The
/// `permission_strings_match_object_and_action` test pins every generated
/// `PERMISSION` to `OBJECT` and `ACTION`, so the literals cannot drift.
macro_rules! permissions {
    ($object:expr, $object_lit:literal; $( $(#[$meta:meta])* $name:ident, $action:expr, $action_lit:literal; )*) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy)]
            pub struct $name;

            impl Permission for $name {
                const OBJECT: Object = $object;
                const ACTION: Action = $action;
                const PERMISSION: &'static str = concat!($object_lit, ":", $action_lit);
            }
        )*
    };
}

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
    Event,
}

impl Object {
    /// The string form used in permission names, e.g. `"artifact"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Artifact => "artifact",
            Self::Task => "task",
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
    /// The string form used in permission names, e.g. `"read"`.
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

/// A built-in role. Stored as its lowercase name in the `role_assignment`
/// table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

impl Role {
    /// The string form stored in the database.
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

/// A role holder: a user actor or an API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subject {
    /// A user or service actor.
    Actor(ActorId),
    /// An API key used by headless clients.
    ApiKey(ApiKeyId),
}

impl Subject {
    /// The storage-level kind of this subject.
    pub const fn kind(self) -> SubjectKind {
        match self {
            Self::Actor(_) => SubjectKind::Actor,
            Self::ApiKey(_) => SubjectKind::ApiKey,
        }
    }

    /// The subject's row id, interpreted per [`Subject::kind`].
    pub const fn db_id(self) -> i64 {
        match self {
            Self::Actor(id) => id.get(),
            Self::ApiKey(id) => id.get(),
        }
    }
}

/// A single grantable permission: one [`Action`] on one [`Object`].
///
/// Implemented only by the marker types in the [`artifact`], [`task`],
/// [`task_schedule`], [`log`], [`user`], [`api_key`], and [`setting`]
/// modules. [`PERMISSION`](Permission::PERMISSION) is the `"object:action"`
/// string published in the [`REQUIRED_PERMISSION_EXTENSION`] `OpenAPI`
/// extension.
pub trait Permission {
    /// The object this permission operates on.
    const OBJECT: Object;
    /// The action this permission performs.
    const ACTION: Action;
    /// The `"object:action"` string used in the `OpenAPI` extension.
    const PERMISSION: &'static str;
}

/// Permissions on artifacts.
pub mod artifact {
    use super::{Action, Object, Permission};

    permissions!(
        Object::Artifact, "artifact";
        /// Guards reading artifact catalog metadata.
        Read, Action::Read, "read";
        /// Guards downloading artifact blobs, which may contain secrets such
        /// as TLS private keys.
        Download, Action::Download, "download";
        /// Guards pushing new or updated artifact contents.
        Write, Action::Write, "write";
        /// Guards evicting artifacts from the store.
        Evict, Action::Evict, "evict";
    );
}

/// Permissions on tasks.
pub mod task {
    use super::{Action, Object, Permission};

    permissions!(
        Object::Task, "task";
        /// Guards reading tasks.
        Read, Action::Read, "read";
        /// Guards enqueueing new tasks.
        Create, Action::Create, "create";
        /// Guards cancelling running tasks.
        Cancel, Action::Cancel, "cancel";
    );
}

/// Permissions on task schedules.
pub mod task_schedule {
    use super::{Action, Object, Permission};

    permissions!(
        Object::TaskSchedule, "task-schedule";
        /// Guards reading task schedules.
        Read, Action::Read, "read";
        /// Guards creating task schedules.
        Create, Action::Create, "create";
        /// Guards updating task schedules.
        Update, Action::Update, "update";
        /// Guards deleting task schedules.
        Delete, Action::Delete, "delete";
    );
}

/// Permissions on logs.
pub mod log {
    use super::{Action, Object, Permission};

    permissions!(
        Object::Log, "log";
        /// Guards reading logs.
        Read, Action::Read, "read";
    );
}

/// Permissions on domain events.
pub mod event {
    use super::{Action, Object, Permission};

    permissions!(
        Object::Event, "event";
        /// Guards reading domain events.
        Read, Action::Read, "read";
    );
}

/// Permissions on users.
pub mod user {
    use super::{Action, Object, Permission};

    permissions!(
        Object::User, "user";
        /// Guards listing or reading users.
        Read, Action::Read, "read";
        /// Guards creating users.
        Create, Action::Create, "create";
        /// Guards deleting users.
        Delete, Action::Delete, "delete";
    );
}

/// Permissions on API keys.
pub mod api_key {
    use super::{Action, Object, Permission};

    permissions!(
        Object::ApiKey, "api-key";
        /// Guards creating API keys.
        Create, Action::Create, "create";
        /// Guards deleting API keys.
        Delete, Action::Delete, "delete";
    );
}

/// Permissions on settings.
pub mod setting {
    use super::{Action, Object, Permission};

    permissions!(
        Object::Setting, "setting";
        /// Guards reading settings.
        Read, Action::Read, "read";
        /// Guards updating settings.
        Update, Action::Update, "update";
    );
}

/// Proof that the PDP granted one permission.
///
/// A `Permit<P>` value can only be minted by the PDP
/// ([`AuthHandle::require`](crate::AuthHandle::require)) after a positive
/// decision for `P`. There is no public constructor, so a permit cannot be
/// forged. Any function that demands a `Permit<P>` argument therefore cannot
/// run unless authorization for `P` was granted on the call path.
///
/// A permit carries no runtime data; it is a zero-sized proof.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct Permit<P> {
    _proof: PhantomData<fn() -> P>,
}

impl<P: Permission> Permit<P> {
    /// Mints a permit. Crate-private: only the PDP mints permits.
    pub(crate) const fn mint() -> Self {
        Self {
            _proof: PhantomData,
        }
    }
}

/// The role matrix: whether `role` grants the `object:action` permission.
///
/// Admin is the superuser and grants everything. Operator grants full
/// operational access but no user or API key management. Viewer grants
/// read-only access to non-sensitive data; it notably does not grant artifact
/// downloads, which would expose secrets such as TLS private keys stored in
/// artifact blobs.
///
/// The operator and viewer matrices are exhaustive over every `Object` and
/// `Action` pair and contain no wildcard arm. Adding an `Object` or `Action`
/// variant therefore breaks compilation here and forces an explicit grant
/// decision for every role. Nothing is ever granted by default.
pub const fn role_allows(role: Role, object: Object, action: Action) -> bool {
    match role {
        Role::Admin => true,
        Role::Operator => operator_allows(object, action),
        Role::Viewer => viewer_allows(object, action),
    }
}

/// Operator's grants, exhaustive over every `Object` x `Action` pair.
const fn operator_allows(object: Object, action: Action) -> bool {
    match (object, action) {
        (
            Object::Artifact | Object::Task | Object::TaskSchedule | Object::Setting,
            Action::Read
            | Action::Download
            | Action::Write
            | Action::Evict
            | Action::Create
            | Action::Update
            | Action::Delete
            | Action::Cancel,
        )
        | (Object::Log | Object::Event, Action::Read) => true,
        (
            Object::Log | Object::Event,
            Action::Download
            | Action::Write
            | Action::Evict
            | Action::Create
            | Action::Update
            | Action::Delete
            | Action::Cancel,
        )
        | (
            Object::User | Object::ApiKey,
            Action::Read
            | Action::Download
            | Action::Write
            | Action::Evict
            | Action::Create
            | Action::Update
            | Action::Delete
            | Action::Cancel,
        ) => false,
    }
}

/// Viewer's grants, exhaustive over every `Object` x `Action` pair.
const fn viewer_allows(object: Object, action: Action) -> bool {
    match (object, action) {
        (
            Object::Artifact
            | Object::Task
            | Object::TaskSchedule
            | Object::Log
            | Object::Event
            | Object::Setting,
            Action::Read,
        ) => true,
        (
            Object::Artifact
            | Object::Task
            | Object::TaskSchedule
            | Object::Log
            | Object::Event
            | Object::Setting,
            Action::Download
            | Action::Write
            | Action::Evict
            | Action::Create
            | Action::Update
            | Action::Delete
            | Action::Cancel,
        )
        | (
            Object::User | Object::ApiKey,
            Action::Read
            | Action::Download
            | Action::Write
            | Action::Evict
            | Action::Create
            | Action::Update
            | Action::Delete
            | Action::Cancel,
        ) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every object, in matrix order. The length must track the enum, so
    /// adding a variant breaks compilation here too.
    const OBJECTS: [Object; 8] = [
        Object::Artifact,
        Object::Task,
        Object::TaskSchedule,
        Object::Log,
        Object::User,
        Object::ApiKey,
        Object::Setting,
        Object::Event,
    ];

    /// Every action, in matrix order.
    const ACTIONS: [Action; 8] = [
        Action::Read,
        Action::Download,
        Action::Write,
        Action::Evict,
        Action::Create,
        Action::Update,
        Action::Delete,
        Action::Cancel,
    ];

    /// The security policy spec in readable form. `role_allows` must agree
    /// with this table for every combination.
    fn spec(role: Role, object: Object, action: Action) -> bool {
        match role {
            Role::Admin => true,
            Role::Operator => match object {
                Object::Artifact | Object::Task | Object::TaskSchedule | Object::Setting => true,
                Object::Log | Object::Event => action == Action::Read,
                Object::User | Object::ApiKey => false,
            },
            Role::Viewer => match object {
                Object::Artifact
                | Object::Task
                | Object::TaskSchedule
                | Object::Log
                | Object::Event
                | Object::Setting => action == Action::Read,
                Object::User | Object::ApiKey => false,
            },
        }
    }

    /// Pins the full current matrix. This test is the security policy spec;
    /// any change to `role_allows` must change it here first.
    #[test]
    fn role_matrix_matches_spec() {
        for role in Role::ALL {
            for object in OBJECTS {
                for action in ACTIONS {
                    assert_eq!(
                        role_allows(role, object, action),
                        spec(role, object, action),
                        "{role} mismatch for {object}:{action}"
                    );
                }
            }
        }
    }

    #[test]
    fn permission_strings_match_object_and_action() {
        fn check<P: Permission>() {
            assert_eq!(
                P::PERMISSION,
                format!("{}:{}", P::OBJECT.as_str(), P::ACTION.as_str())
            );
        }
        check::<artifact::Read>();
        check::<artifact::Download>();
        check::<artifact::Write>();
        check::<artifact::Evict>();
        check::<task::Read>();
        check::<task::Create>();
        check::<task::Cancel>();
        check::<task_schedule::Read>();
        check::<task_schedule::Create>();
        check::<task_schedule::Update>();
        check::<task_schedule::Delete>();
        check::<log::Read>();
        check::<user::Read>();
        check::<user::Create>();
        check::<user::Delete>();
        check::<api_key::Create>();
        check::<api_key::Delete>();
        check::<setting::Read>();
        check::<setting::Update>();
    }

    #[test]
    fn subject_maps_to_kind_and_db_id() {
        let actor = Subject::Actor(ActorId::from(7));
        assert_eq!(actor.kind(), SubjectKind::Actor);
        assert_eq!(actor.db_id(), 7);

        let key = Subject::ApiKey(ApiKeyId::from(3));
        assert_eq!(key.kind(), SubjectKind::ApiKey);
        assert_eq!(key.db_id(), 3);
    }
}
