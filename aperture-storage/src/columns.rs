//! Compile-time validated column lists for SELECT construction and index
//! lookup.

use std::fmt;

use turso::Row;

use crate::error::{Result, database};
use crate::sql::FromSql;

/// A set of column names for a SELECT clause, validated at construction time.
///
/// All names must be simple lowercase ASCII identifiers (letters, digits,
/// underscore). This guarantees they are safe to interpolate into SQL without
/// quoting. Use [`Columns::index_of`] to look up a column's position by name,
/// so row decoders stay in sync if columns are reordered.
pub(crate) struct Columns {
    names: &'static [&'static str],
}

impl Columns {
    /// Creates a column list from a slice of static strings, validating that
    /// each is a simple lowercase ASCII identifier. Panics at compile time if
    /// any name is invalid.
    pub const fn new(names: &'static [&'static str]) -> Self {
        let mut i = 0;
        while i < names.len() {
            validate_column_name(names[i]);
            i += 1;
        }
        Self { names }
    }

    /// Returns the index of `name` in the column list. Panics if not found.
    pub const fn index_of(&self, name: &str) -> usize {
        let mut i = 0;
        while i < self.names.len() {
            // Using eq_ignore_ascii_case here because it's const-stable.
            if self.names[i].eq_ignore_ascii_case(name) {
                return i;
            }
            i += 1;
        }
        panic!("column not found in column list");
    }

    /// Looks up `name` and extracts a value of type `T` from `row` at that
    /// index.
    pub fn extract<T: FromSql>(&self, row: &Row, name: &str) -> Result<T> {
        let idx = self.index_of(name);
        let value = row.get_value(idx).map_err(database)?;
        T::from_sql(value, idx)
    }

    pub const fn len(&self) -> usize {
        self.names.len()
    }
}

impl fmt::Display for Columns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for name in self.names {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            f.write_str(name)?;
        }
        Ok(())
    }
}

const fn validate_column_name(name: &str) {
    let bytes = name.as_bytes();
    assert!(!bytes.is_empty(), "column name must not be empty");
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        assert!(
            matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'),
            "column name must be lowercase ASCII identifier"
        );
        i += 1;
    }
}
