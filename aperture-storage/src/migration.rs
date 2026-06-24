//! Hand-rolled, forward-only schema migrations.
//!
//! The schema version is tracked with `PRAGMA user_version`. Each migration
//! lives in its own submodule and exposes a `SQL` constant.

use turso::{Connection, Value};

use self::m0001_initial::SQL as M0001_INITIAL;
use crate::error::{Result, StorageError, database};
use crate::macros::sql;

mod m0001_initial;

/// Ordered list of migrations. Index `i` upgrades the schema from version `i`
/// to version `i + 1`.
const MIGRATIONS: &[&str] = &[M0001_INITIAL];

/// Applies all pending migrations to the database.
pub(crate) async fn run(connection: &Connection) -> Result<()> {
    let current = current_version(connection).await?;
    let target = MIGRATIONS.len() as i64;
    if current > target {
        return Err(StorageError::Migration(format!(
            "database schema version {current} is newer than the supported {target}"
        )));
    }
    for (index, statements) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        apply(connection, statements, index as i64 + 1).await?;
    }
    Ok(())
}

async fn current_version(connection: &Connection) -> Result<i64> {
    let mut rows = connection
        .query(sql!(PRAGMA user_version), ())
        .await
        .map_err(database)?;
    match rows.next().await.map_err(database)? {
        Some(row) => match row.get_value(0).map_err(database)? {
            Value::Integer(version) => Ok(version),
            other => Err(StorageError::Migration(format!(
                "user_version is not an integer: {other:?}"
            ))),
        },
        None => Ok(0),
    }
}

async fn apply(connection: &Connection, statements: &str, version: i64) -> Result<()> {
    connection
        .execute(sql!(BEGIN), ())
        .await
        .map_err(database)?;
    if let Err(err) = apply_inner(connection, statements, version).await {
        let _ = connection.execute(sql!(ROLLBACK), ()).await;
        return Err(err);
    }
    connection
        .execute(sql!(COMMIT), ())
        .await
        .map_err(database)?;
    Ok(())
}

async fn apply_inner(connection: &Connection, statements: &str, version: i64) -> Result<()> {
    connection
        .execute_batch(statements)
        .await
        .map_err(database)?;
    connection
        .pragma_update("user_version", version)
        .await
        .map_err(database)?;
    Ok(())
}
