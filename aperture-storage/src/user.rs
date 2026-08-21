//! User credentials: usernames, password hashes, and the password-change flag.

use jiff::Timestamp;
use turso::{Connection, Row, params_from_iter};

use crate::actor::ActorId;
use crate::error::{Result, StorageError};
use crate::macros::{db_id, sql};
use crate::page::{CursorValue, Keyset, ListQuery, Listing, Order, Page, Paginator};
use crate::query::Filters;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub(crate) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Creates a new user and returns the full record.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the insert fails.
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
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
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
            Some(row) => Ok(Some(User::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns the user with `id`, if one exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
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
            Some(row) => Ok(Some(User::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns the user associated with `actor_id`, if one exists.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the row cannot be decoded.
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
            Some(row) => Ok(Some(User::try_from(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists users, paginated by username (ascending by default).
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query or cursor is invalid, or a row
    /// cannot be decoded.
    #[tracing::instrument(level = "info", skip(self, query))]
    pub async fn list(&self, query: &ListQuery) -> Result<Page<User>> {
        let paginator = Paginator::new(query, Order::Asc, Listing::Users)?;
        let keyset = Keyset::unique(col::USERNAME, paginator.query_order());

        let mut filters = Filters::new();
        filters.keyset(&keyset, &paginator);

        let sql_str = format!(
            sql!(SELECT {cols} FROM users {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = USER_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );
        let mut rows = self
            .connection
            .query(&sql_str, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut users = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            users.push(User::try_from(&row)?);
        }
        Ok(paginator.finish(users, |user| (CursorValue::Text(user.username.clone()), 0)))
    }

    /// Updates the password hash and the password-change-required timestamp.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the update fails.
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
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Database` if the delete fails.
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
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if the query fails or the count cannot be read.
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

impl TryFrom<&Row> for User {
    type Error = StorageError;

    fn try_from(row: &Row) -> Result<Self> {
        Ok(Self {
            id: USER_COLUMNS.extract(row, col::ID)?,
            actor_id: USER_COLUMNS.extract(row, col::ACTOR_ID)?,
            username: USER_COLUMNS.extract(row, col::USERNAME)?,
            password_hash: USER_COLUMNS.extract(row, col::PASSWORD_HASH)?,
            password_change_required_at: USER_COLUMNS
                .extract(row, col::PASSWORD_CHANGE_REQUIRED_AT)?,
            created_at: USER_COLUMNS.extract(row, col::CREATED_AT)?,
        })
    }
}
