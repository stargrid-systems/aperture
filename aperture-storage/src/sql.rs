//! Traits for converting between domain types and turso [`Value`]s.
//!
//! Every type that goes into or comes out of the database implements [`ToSql`]
//! and/or [`FromSql`]. The blanket impl for [`Option<T>`] handles NULL
//! automatically: [`Option<T>`] maps to `NULL` when `None`, and [`ToSql`] or
//! [`FromSql`] on `T` when `Some`.

use turso::{Row, Value};

pub use self::columns::Columns;
use crate::error::{Result, StorageError};

mod columns;
mod domain;
mod integer;
mod json;
mod text;

/// Convert a value to a database [`Value`] for binding.
pub(crate) trait ToSql {
    fn to_sql(&self) -> Value;
}

/// Convert a database [`Value`] back to a domain type.
///
/// `idx` is the column index, used for error messages.
pub(crate) trait FromSql: Sized {
    fn from_sql(value: Value, idx: usize) -> Result<Self>;
}

/// Extracts a value at `idx` from `row`.
///
/// Shortcut for `T::from_sql(row.get_value(idx)?, idx)`.
pub(crate) fn get<T: FromSql>(row: &Row, idx: usize) -> Result<T> {
    let value = row.get_value(idx).map_err(StorageError::from_turso)?;
    T::from_sql(value, idx)
}

impl<T: ToSql + ?Sized> ToSql for &T {
    fn to_sql(&self) -> Value {
        (**self).to_sql()
    }
}

impl<T: ToSql> ToSql for Option<T> {
    fn to_sql(&self) -> Value {
        match self {
            Some(v) => v.to_sql(),
            None => Value::Null,
        }
    }
}

impl<T: FromSql> FromSql for Option<T> {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        match value {
            Value::Null => Ok(None),
            other => T::from_sql(other, idx).map(Some),
        }
    }
}
