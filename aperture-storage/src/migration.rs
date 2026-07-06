//! Hand-rolled, forward-only schema migrations.
//!
//! The schema version is tracked with `PRAGMA user_version`. Each migration
//! lives in its own submodule and exposes a `SQL` constant.

use turso::{Connection, Value};

use self::m0001_initial::SQL as M0001_INITIAL;
use crate::error::{Result, StorageError, database};
use crate::macros::sql;

mod m0001_initial;

/// Schema version that exists before the first migration in [`MIGRATIONS`].
///
/// Normally 0. When old migrations are squashed into a single baseline, bump
/// this to the version that baseline represents. The first entry then upgrades
/// `BASE_VERSION` to `BASE_VERSION + 1` instead of `0` to `1`.
const BASE_VERSION: i64 = 0;

/// Ordered list of migrations. Index `i` upgrades the schema from version
/// `BASE_VERSION + i` to `BASE_VERSION + i + 1`.
const MIGRATIONS: &[&str] = &[M0001_INITIAL];

/// Applies all pending migrations to the database.
pub(crate) async fn run(connection: &Connection) -> Result<()> {
    let current = current_version(connection).await?;
    let target = BASE_VERSION + MIGRATIONS.len() as i64;
    if current > target {
        return Err(StorageError::SchemaTooNew { current, target });
    }
    // A non-empty database below the baseline predates a squash and cannot jump
    // straight to it. It must be upgraded by an older release first.
    if current != 0 && current < BASE_VERSION {
        return Err(StorageError::SchemaTooOld {
            current,
            baseline: BASE_VERSION,
        });
    }
    for (index, statements) in MIGRATIONS.iter().enumerate() {
        let version = BASE_VERSION + index as i64 + 1;
        if version <= current {
            continue;
        }
        apply(connection, statements, version).await?;
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
            other => Err(StorageError::InvalidUserVersion { value: other }),
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
