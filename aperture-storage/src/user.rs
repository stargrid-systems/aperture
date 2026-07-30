//! User credentials: usernames, password hashes, and the password-change flag.

use jiff::Timestamp;
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::secret::PasswordHash;
use crate::sql::{Columns, ToSql, get};

db_id! {
    /// Primary key of a row in the `users` table.
    pub struct UserId;
}

mod col {
    pub const ACTOR_ID: &str = "actor_id";
    pub const CREATED_AT: &str = "created_at";
    pub const ID: &str = "id";
    pub const PASSWORD_CHANGE_REQUIRED_AT: &str = "password_change_required_at";
    pub const PASSWORD_HASH: &str = "password_hash";
    pub const USERNAME: &str = "username";
}

const USER_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::ACTOR_ID,
    col::USERNAME,
    col::PASSWORD_HASH,
    col::PASSWORD_CHANGE_REQUIRED_AT,
    col::CREATED_AT,
]);

/// A registered user with credentials.
#[derive(Debug, Clone, PartialEq)]
pub struct User {
    /// Store-assigned id.
    pub id: UserId,
    /// The actor this user authenticates as.
    pub actor_id: ActorId,
    /// Unique login name.
    pub username: String,
    /// Argon2 password hash.
    pub password_hash: PasswordHash,
    /// When a password change was required, if applicable.
    pub password_change_required_at: Option<Timestamp>,
    /// When the user was created.
    pub created_at: Timestamp,
}

/// Repository over the users table.
pub struct UserRepository {
    connection: Connection,
}

impl UserRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new user and returns the full record.
    #[tracing::instrument(level = "info", skip(self, password_hash))]
    pub async fn create(
        &self,
        actor_id: ActorId,
        username: &str,
        password_hash: &PasswordHash,
        password_change_required_at: Option<Timestamp>,
        created_at: Timestamp,
    ) -> Result<User> {
        let params = params_from_iter([
            actor_id.to_sql(),
            username.to_sql(),
            password_hash.to_sql(),
            password_change_required_at.to_sql(),
            created_at.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO users (actor_id, username, password_hash, password_change_required_at, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        let id = UserId::from(self.connection.last_insert_rowid());
        Ok(User {
            id,
            actor_id,
            username: username.to_owned(),
            password_hash: password_hash.clone(),
            password_change_required_at,
            created_at,
        })
    }

    /// Returns the user with `username`, if one exists.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM users WHERE username = ?1),
            cols = USER_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([username.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns the user with `id`, if one exists.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get(&self, id: UserId) -> Result<Option<User>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM users WHERE id = ?1),
            cols = USER_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns the user associated with `actor_id`, if one exists.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn find_by_actor_id(&self, actor_id: ActorId) -> Result<Option<User>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM users WHERE actor_id = ?1),
            cols = USER_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter([actor_id.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists all users, ordered by username.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn list(&self) -> Result<Vec<User>> {
        let sql_str = format!(
            sql!(SELECT {cols} FROM users ORDER BY username),
            cols = USER_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql_str, ())
            .await
            .map_err(StorageError::from_turso)?;
        let mut users = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            users.push(row_to_user(&row)?);
        }
        Ok(users)
    }

    /// Updates the password hash and the password-change-required timestamp.
    #[tracing::instrument(level = "info", skip(self, password_hash))]
    pub async fn update_password(
        &self,
        id: UserId,
        password_hash: &PasswordHash,
        password_change_required_at: Option<Timestamp>,
    ) -> Result<()> {
        self.connection
            .execute(
                sql!(
                    UPDATE users
                    SET password_hash = ?1, password_change_required_at = ?2
                    WHERE id = ?3
                ),
                params_from_iter([
                    password_hash.to_sql(),
                    password_change_required_at.to_sql(),
                    id.to_sql(),
                ]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Deletes the user with `id`. Does nothing if absent.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete(&self, id: UserId) -> Result<()> {
        self.connection
            .execute(
                sql!(DELETE FROM users WHERE id = ?1),
                params_from_iter([id.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Returns how many users exist. Used at bootstrap to detect first run.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn count(&self) -> Result<i64> {
        let mut rows = self
            .connection
            .query(sql!(SELECT COUNT(*) FROM users), ())
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(get(&row, 0)?),
            None => Ok(0),
        }
    }
}

fn row_to_user(row: &Row) -> Result<User> {
    Ok(User {
        id: USER_COLUMNS.extract(row, col::ID)?,
        actor_id: USER_COLUMNS.extract(row, col::ACTOR_ID)?,
        username: USER_COLUMNS.extract(row, col::USERNAME)?,
        password_hash: USER_COLUMNS.extract(row, col::PASSWORD_HASH)?,
        password_change_required_at: USER_COLUMNS.extract(row, col::PASSWORD_CHANGE_REQUIRED_AT)?,
        created_at: USER_COLUMNS.extract(row, col::CREATED_AT)?,
    })
}
