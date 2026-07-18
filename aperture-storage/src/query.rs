//! SQL fragment builders that keep conditions and bind params in lockstep.

use std::fmt::{self, Write};

use turso::Value;

use crate::page::{Keyset, Paginator};
use crate::sql::ToSql;

/// Builds the `WHERE` body of a listing, keeping conditions and their bind
/// params in lockstep so placeholder numbers can never drift. Column names come
/// from the calling query, never user input, since they are written verbatim.
pub(crate) struct Filters {
    sql: String,
    params: Vec<Value>,
}

impl Filters {
    pub(crate) fn new() -> Self {
        Self {
            sql: String::new(),
            params: Vec::new(),
        }
    }

    fn separator(&mut self) {
        if !self.sql.is_empty() {
            self.sql.push_str(" AND ");
        }
    }

    /// Adds a condition with no bind params (a fixed predicate).
    pub(crate) fn raw(&mut self, condition: &str) {
        self.separator();
        self.sql.push_str(condition);
    }

    /// Adds `column = ?` bound to `value`.
    pub(crate) fn eq_text(&mut self, column: &str, value: &str) {
        self.params.push(Value::Text(value.to_owned()));
        self.separator();
        let _ = write!(self.sql, "{column} = ?{}", self.params.len());
    }

    /// Like [`eq_text`](Self::eq_text), but skips the condition when `None`.
    pub(crate) fn eq_text_opt(&mut self, column: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.eq_text(column, value);
        }
    }

    /// Adds `column = ?` bound to `value`.
    pub(crate) fn eq_int(&mut self, column: &str, value: i64) {
        self.params.push(Value::Integer(value));
        self.separator();
        let _ = write!(self.sql, "{column} = ?{}", self.params.len());
    }

    /// Like [`eq_int`](Self::eq_int), but skips the condition when `None`.
    pub(crate) fn eq_int_opt(&mut self, column: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.eq_int(column, value);
        }
    }

    /// Adds `column IN (?, ?, ...)` bound to `values`, or nothing when empty.
    pub(crate) fn one_of<'a>(&mut self, column: &str, mut values: impl Iterator<Item = &'a str>) {
        let first = match values.next() {
            None => return,
            Some(v) => {
                self.separator();
                self.params.push(Value::Text(v.to_owned()));
                self.params.len()
            }
        };
        for value in values {
            self.params.push(Value::Text(value.to_owned()));
        }
        let last = self.params.len();
        let placeholders = (first..=last)
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(self.sql, "{column} IN ({placeholders})");
    }

    /// Adds `CAST(json_extract(column, '$.<path>') AS TEXT) = ?`, matching a
    /// field inside the JSON stored in `column`. `column` is a fixed identifier
    /// from the caller. `path` and `value` are user input and are always bound,
    /// never interpolated, so the path cannot inject SQL. The `CAST` lets a
    /// numeric or boolean field match a text value too.
    pub(crate) fn json_path_eq(&mut self, column: &str, path: &str, value: &str) {
        self.separator();
        self.params.push(Value::Text(format!("$.{path}")));
        let path_ph = self.params.len();
        self.params.push(Value::Text(value.to_owned()));
        let value_ph = self.params.len();
        let _ = write!(
            self.sql,
            "CAST(json_extract({column}, ?{path_ph}) AS TEXT) = ?{value_ph}"
        );
    }

    /// Adds `column LIKE ?` matching `value` as a literal substring (wildcards
    /// escaped).
    pub(crate) fn like(&mut self, column: &str, value: &str) {
        self.params
            .push(Value::Text(format!("%{}%", EscapeLike(value))));
        self.separator();
        let _ = write!(self.sql, "{column} LIKE ?{} ESCAPE '\\'", self.params.len());
    }

    /// Like [`like`](Self::like), but skips the condition when `None`.
    pub(crate) fn like_opt(&mut self, column: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.like(column, value);
        }
    }

    /// Adds `(c1 LIKE ? ESCAPE '\' OR c2 LIKE ? ESCAPE '\' ...)` where each
    /// column references the same reused param value. Wildcards in `value` are
    /// escaped, so it matches as a literal substring.
    pub(crate) fn like_any(&mut self, columns: &[&str], value: &str) {
        self.params
            .push(Value::Text(format!("%{}%", EscapeLike(value))));
        self.separator();
        let placeholder = self.params.len();
        let mut first = true;
        self.sql.push('(');
        for col in columns {
            if !first {
                self.sql.push_str(" OR ");
            }
            first = false;
            let _ = write!(self.sql, "{col} LIKE ?{placeholder} ESCAPE '\\'");
        }
        self.sql.push(')');
    }

    /// Like [`like_any`](Self::like_any), but skips the condition when `None`.
    pub(crate) fn like_any_opt(&mut self, columns: &[&str], value: Option<&str>) {
        if let Some(value) = value {
            self.like_any(columns, value);
        }
    }

    /// Adds `column >= ?` bound to the integer `value`.
    pub(crate) fn gte_int(&mut self, column: &str, value: i64) {
        self.params.push(Value::Integer(value));
        self.separator();
        let _ = write!(self.sql, "{column} >= ?{}", self.params.len());
    }

    /// Like [`gte_int`](Self::gte_int), but skips the condition when `None`.
    pub(crate) fn gte_int_opt(&mut self, column: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.gte_int(column, value);
        }
    }

    /// Adds `column <= ?` bound to the integer `value`.
    pub(crate) fn lte_int(&mut self, column: &str, value: i64) {
        self.params.push(Value::Integer(value));
        self.separator();
        let _ = write!(self.sql, "{column} <= ?{}", self.params.len());
    }

    /// Like [`lte_int`](Self::lte_int), but skips the condition when `None`.
    pub(crate) fn lte_int_opt(&mut self, column: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.lte_int(column, value);
        }
    }

    /// Adds the keyset resume condition for `paginator`, if it has a cursor.
    pub(crate) fn keyset(&mut self, keyset: &Keyset, paginator: &Paginator) {
        let (condition, params) = keyset.condition(paginator.cursor(), self.params.len() + 1);
        if !condition.is_empty() {
            self.params.extend(params);
            self.separator();
            self.sql.push_str(&condition);
        }
    }

    /// The `WHERE` clause, or an empty string when there are no conditions.
    pub(crate) fn where_clause(&self) -> String {
        if self.sql.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.sql)
        }
    }

    /// The bind params, in placeholder order.
    pub(crate) fn into_params(self) -> Vec<Value> {
        self.params
    }
}

/// Escapes the LIKE wildcards `%` and `_` (and the escape char itself) while
/// formatting, so a user-supplied substring matches literally. Escaping into
/// the formatter avoids an intermediate allocation when used in a `format!`.
/// Pair with `ESCAPE '\'` in the SQL.
pub(crate) struct EscapeLike<'a>(pub(crate) &'a str);

impl fmt::Display for EscapeLike<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in self.0.chars() {
            if matches!(ch, '\\' | '%' | '_') {
                f.write_char('\\')?;
            }
            f.write_char(ch)?;
        }
        Ok(())
    }
}

pub(crate) struct Assignments {
    sql: String,
    params: Vec<Value>,
}

impl Assignments {
    pub(crate) fn new() -> Self {
        Self {
            sql: String::new(),
            params: Vec::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub(crate) fn set(&mut self, column: &str, value: &impl ToSql) {
        self.params.push(value.to_sql());
        if !self.sql.is_empty() {
            self.sql.push_str(", ");
        }
        let _ = write!(self.sql, "{column} = ?{}", self.params.len());
    }

    pub(crate) fn set_opt(&mut self, column: &str, value: Option<&impl ToSql>) {
        if let Some(value) = value {
            self.set(column, value);
        }
    }

    pub(crate) fn set_clause(&self) -> &str {
        &self.sql
    }

    pub(crate) fn into_params(self) -> Vec<Value> {
        self.params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Int(i64);
    impl ToSql for Int {
        fn to_sql(&self) -> Value {
            Value::Integer(self.0)
        }
    }

    #[test]
    fn assignments_empty_when_nothing_pushed() {
        let assignments = Assignments::new();
        assert!(assignments.is_empty());
        assert_eq!(assignments.set_clause(), "");
    }

    #[test]
    fn assignments_pairs_clause_with_params() {
        let mut assignments = Assignments::new();
        assignments.set("interval_us", &Int(60_000_000));
        assignments.set("enabled", &Int(1));
        assert!(!assignments.is_empty());
        assert_eq!(assignments.set_clause(), "interval_us = ?1, enabled = ?2");
        assert_eq!(
            assignments.into_params(),
            vec![Value::Integer(60_000_000), Value::Integer(1)]
        );
    }

    #[test]
    fn assignments_skips_none() {
        let mut assignments = Assignments::new();
        assignments.set_opt("interval_us", None::<&Int>);
        assignments.set("enabled", &Int(0));
        assert_eq!(assignments.set_clause(), "enabled = ?1");
        assert_eq!(assignments.into_params(), vec![Value::Integer(0)]);
    }
}
