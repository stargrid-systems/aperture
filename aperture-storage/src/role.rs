//! Role assignments: the only dynamic authorization fact.
//!
//! The `role_assignment` table records that a subject (an actor or an API
//! key actor) holds a role. The permission grants behind each role live in
//! code, not in the database, so the storage layer never sees permissions,
//! only the subject -> role mapping.

use std::fmt;
use std::str::FromStr;

use turso::{Connection, Row, Value, params_from_iter};

use crate::error::{Result, StorageError};
use crate::macros::sql;
use crate::sql::{ToSql, get};

/// Which type of subject holds a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectKind {
    /// An actor row.
    Actor,
    /// An API key used by headless clients.
    ApiKey,
}

impl SubjectKind {
    pub const fn as_db(self) -> &'static str {
        match self {
            Self::Actor => "actor",
            Self::ApiKey => "api-key",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "actor" => Ok(Self::Actor),
            "api-key" => Ok(Self::ApiKey),
            other => Err(StorageError::UnknownSubjectKind(other.to_owned())),
        }
    }
}

impl fmt::Display for SubjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db())
    }
}

impl FromStr for SubjectKind {
    type Err = StorageError;
    fn from_str(s: &str) -> Result<Self> {
        Self::from_db(s)
    }
}

impl ToSql for SubjectKind {
    fn to_sql(&self) -> Value {
        Value::Text(self.as_db().to_owned())
    }
}

/// One subject's membership in a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssignment {
    /// What kind of subject this is.
    pub kind: SubjectKind,
    /// The subject's id, interpreted per `kind`.
    pub subject_id: i64,
    /// The role the subject holds.
    pub role: String,
}

/// Repository over the `role_assignment` table.
pub struct RoleAssignmentRepository {
    connection: Connection,
}

impl RoleAssignmentRepository {
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Grants `role` to the subject. Does nothing if the subject already
    /// holds it.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
    #[tracing::instrument(level = "info", skip(self, role))]
    pub async fn insert(&self, kind: SubjectKind, subject_id: i64, role: &str) -> Result<()> {
        self.connection
            .execute(
                sql!(
                    INSERT OR IGNORE INTO role_assignment (subject_kind, subject_id, role)
                    VALUES (?1, ?2, ?3)
                ),
                params_from_iter([kind.to_sql(), subject_id.to_sql(), role.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Removes every role held by the subject. Returns how many were removed.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the delete fails.
    ///
    /// # Panics
    ///
    /// Never panics in practice. The affected row count fits `usize`.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete_for_subject(&self, kind: SubjectKind, subject_id: i64) -> Result<usize> {
        let affected = self
            .connection
            .execute(
                sql!(DELETE FROM role_assignment WHERE subject_kind = ?1 AND subject_id = ?2),
                params_from_iter([kind.to_sql(), subject_id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(usize::try_from(affected).expect("row count fits usize"))
    }

    /// Lists the roles held by the subject, ordered by role name.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn roles_for(&self, kind: SubjectKind, subject_id: i64) -> Result<Vec<String>> {
        let mut rows = self
            .connection
            .query(
                sql!(
                    SELECT role FROM role_assignment
                    WHERE subject_kind = ?1 AND subject_id = ?2
                    ORDER BY role
                ),
                params_from_iter([kind.to_sql(), subject_id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        let mut roles = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            roles.push(get(&row, 0)?);
        }
        Ok(roles)
    }

    /// Lists every subject holding `role`, ordered by subject kind then
    /// subject id.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or a row cannot be decoded.
    #[tracing::instrument(level = "info", skip(self, role))]
    pub async fn subjects_with_role(&self, role: &str) -> Result<Vec<RoleAssignment>> {
        let mut rows = self
            .connection
            .query(
                sql!(
                    SELECT subject_kind, subject_id, role FROM role_assignment
                    WHERE role = ?1
                    ORDER BY subject_kind, subject_id
                ),
                params_from_iter([role.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        let mut assignments = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            assignments.push(Self::get(&row)?);
        }
        Ok(assignments)
    }

    /// Decodes one row of the `role_assignment` table.
    fn get(row: &Row) -> Result<RoleAssignment> {
        let kind: String = get(row, 0)?;
        Ok(RoleAssignment {
            kind: SubjectKind::from_db(&kind)?,
            subject_id: get(row, 1)?,
            role: get(row, 2)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Storage;

    #[tokio::test]
    async fn insert_and_roles_for_roundtrip() {
        let storage = Storage::open(":memory:").await.unwrap();
        let repo = storage.role_assignments().unwrap();
        repo.insert(SubjectKind::Actor, 7, "viewer").await.unwrap();
        repo.insert(SubjectKind::ApiKey, 3, "admin").await.unwrap();

        let roles = repo.roles_for(SubjectKind::Actor, 7).await.unwrap();
        assert_eq!(roles, vec!["viewer".to_owned()]);
        let roles = repo.roles_for(SubjectKind::ApiKey, 3).await.unwrap();
        assert_eq!(roles, vec!["admin".to_owned()]);
        let roles = repo.roles_for(SubjectKind::Actor, 8).await.unwrap();
        assert!(roles.is_empty());
    }

    /// The primary key makes a repeated grant a no-op.
    #[tokio::test]
    async fn duplicate_insert_is_a_no_op() {
        let storage = Storage::open(":memory:").await.unwrap();
        let repo = storage.role_assignments().unwrap();
        repo.insert(SubjectKind::Actor, 7, "viewer").await.unwrap();
        repo.insert(SubjectKind::Actor, 7, "viewer").await.unwrap();

        let roles = repo.roles_for(SubjectKind::Actor, 7).await.unwrap();
        assert_eq!(roles, vec!["viewer".to_owned()]);
    }

    #[tokio::test]
    async fn delete_for_subject_only_touches_given_subject() {
        let storage = Storage::open(":memory:").await.unwrap();
        let repo = storage.role_assignments().unwrap();
        repo.insert(SubjectKind::Actor, 7, "viewer").await.unwrap();
        repo.insert(SubjectKind::Actor, 7, "editor").await.unwrap();
        repo.insert(SubjectKind::Actor, 8, "viewer").await.unwrap();
        repo.insert(SubjectKind::ApiKey, 7, "viewer").await.unwrap();

        let removed = repo
            .delete_for_subject(SubjectKind::Actor, 7)
            .await
            .unwrap();
        assert_eq!(removed, 2);

        assert!(
            repo.roles_for(SubjectKind::Actor, 7)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            repo.roles_for(SubjectKind::Actor, 8).await.unwrap(),
            vec!["viewer".to_owned()]
        );
        assert_eq!(
            repo.roles_for(SubjectKind::ApiKey, 7).await.unwrap(),
            vec!["viewer".to_owned()]
        );
    }

    #[tokio::test]
    async fn subjects_with_role_filters_correctly() {
        let storage = Storage::open(":memory:").await.unwrap();
        let repo = storage.role_assignments().unwrap();
        repo.insert(SubjectKind::ApiKey, 4, "viewer").await.unwrap();
        repo.insert(SubjectKind::Actor, 9, "viewer").await.unwrap();
        repo.insert(SubjectKind::Actor, 2, "viewer").await.unwrap();
        repo.insert(SubjectKind::Actor, 5, "editor").await.unwrap();

        let subjects = repo.subjects_with_role("viewer").await.unwrap();
        assert_eq!(
            subjects,
            vec![
                RoleAssignment {
                    kind: SubjectKind::Actor,
                    subject_id: 2,
                    role: "viewer".to_owned(),
                },
                RoleAssignment {
                    kind: SubjectKind::Actor,
                    subject_id: 9,
                    role: "viewer".to_owned(),
                },
                RoleAssignment {
                    kind: SubjectKind::ApiKey,
                    subject_id: 4,
                    role: "viewer".to_owned(),
                },
            ]
        );
    }
}
