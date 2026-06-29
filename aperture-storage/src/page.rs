//! Cursor-based (keyset) pagination over catalog listings.
//!
//! A listing orders rows by one column plus the unique `id` as a tiebreaker.
//! The cursor carries the last (or first) row's sort value, its id, and the
//! direction to travel. So a single `cursor` value pages either forward or
//! backward, and the caller never needs a separate direction flag. This stays
//! correct even when rows are inserted between page fetches, as long as the sort
//! field and direction do not change.

use std::fmt::Write;

use turso::Value;

use crate::error::{Result, StorageError};

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;

/// Sort direction for a listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl Order {
    fn flip(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }

    fn comparison(self) -> &'static str {
        match self {
            Self::Asc => ">",
            Self::Desc => "<",
        }
    }
}

/// Which way to travel from a cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Rows after the cursor, in base order (the next page).
    After,
    /// Rows before the cursor, in base order (the previous page).
    Before,
}

/// A page of results plus the cursors for the neighbouring pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    /// The rows in this page, in base order.
    pub items: Vec<T>,
    /// Cursor for the next page, or `None` at the end.
    pub next_cursor: Option<String>,
    /// Cursor for the previous page, or `None` at the start.
    pub prev_cursor: Option<String>,
}

/// How much to return and where to resume from.
#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    /// Maximum rows to return. Clamped to `1..=200`. Defaults to 50.
    pub limit: Option<u32>,
    /// Opaque cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction. Each listing applies its own default.
    pub order: Option<Order>,
}

/// The sort key value carried by a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorValue {
    Int(i64),
    Text(String),
}

impl CursorValue {
    fn to_db(&self) -> Value {
        match self {
            Self::Int(int) => Value::Integer(*int),
            Self::Text(text) => Value::Text(text.clone()),
        }
    }
}

/// A decoded keyset position: a row's sort value, its id, and the travel
/// direction baked in when the cursor was issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cursor {
    value: CursorValue,
    id: i64,
    step: Step,
}

impl Cursor {
    fn encode(value: CursorValue, id: i64, step: Step) -> String {
        let mut flags = 0u8;
        if matches!(value, CursorValue::Text(_)) {
            flags |= 0b01;
        }
        if matches!(step, Step::Before) {
            flags |= 0b10;
        }
        let mut buf = vec![flags];
        buf.extend_from_slice(&id.to_be_bytes());
        match value {
            CursorValue::Int(int) => buf.extend_from_slice(&int.to_be_bytes()),
            CursorValue::Text(text) => buf.extend_from_slice(text.as_bytes()),
        }
        to_hex(&buf)
    }

    fn decode(encoded: &str) -> Result<Self> {
        let invalid = || StorageError::Decode(format!("invalid cursor {encoded:?}"));
        let buf = from_hex(encoded).ok_or_else(invalid)?;
        if buf.len() < 9 {
            return Err(invalid());
        }
        let flags = buf[0];
        let id = i64::from_be_bytes(buf[1..9].try_into().expect("8 bytes"));
        let step = if flags & 0b10 != 0 {
            Step::Before
        } else {
            Step::After
        };
        let value = if flags & 0b01 != 0 {
            CursorValue::Text(String::from_utf8(buf[9..].to_vec()).map_err(|_| invalid())?)
        } else {
            if buf.len() != 17 {
                return Err(invalid());
            }
            CursorValue::Int(i64::from_be_bytes(buf[9..17].try_into().expect("8 bytes")))
        };
        Ok(Self { value, id, step })
    }
}

/// Describes how a listing is sorted and builds the SQL for it.
///
/// `column` must be a fixed identifier from the calling query, never user
/// input, since it is interpolated into SQL.
pub(crate) struct Keyset {
    column: &'static str,
    order: Order,
    tiebreak: bool,
}

impl Keyset {
    /// Sorts by `column` (a real column unique per row) with no extra tiebreaker.
    pub(crate) fn unique(column: &'static str, order: Order) -> Self {
        Self {
            column,
            order,
            tiebreak: false,
        }
    }

    /// Sorts by `column`, breaking ties on the unique `id` column.
    pub(crate) fn with_id(column: &'static str, order: Order) -> Self {
        Self {
            column,
            order,
            tiebreak: true,
        }
    }

    /// The `ORDER BY` body (without the keyword).
    pub(crate) fn order_by(&self) -> String {
        let dir = self.order.keyword();
        if self.tiebreak {
            format!("{} {dir}, id {dir}", self.column)
        } else {
            format!("{} {dir}", self.column)
        }
    }

    /// The keyset `WHERE` condition that resumes from `cursor`, using bind
    /// params starting at `first_param`. Empty (and no params) without a cursor.
    fn condition(&self, cursor: Option<&Cursor>, first_param: usize) -> (String, Vec<Value>) {
        let Some(cursor) = cursor else {
            return (String::new(), Vec::new());
        };
        let col = self.column;
        let op = self.order.comparison();
        if self.tiebreak {
            let sql = format!(
                "({col} {op} ?{} OR ({col} = ?{} AND id {op} ?{}))",
                first_param,
                first_param + 1,
                first_param + 2,
            );
            (
                sql,
                vec![
                    cursor.value.to_db(),
                    cursor.value.to_db(),
                    Value::Integer(cursor.id),
                ],
            )
        } else {
            (
                format!("{col} {op} ?{first_param}"),
                vec![cursor.value.to_db()],
            )
        }
    }
}

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

    /// Adds `column = ?` bound to `value`, or nothing when `value` is `None`.
    pub(crate) fn eq_text(&mut self, column: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.params.push(Value::Text(value.to_owned()));
            self.separator();
            let _ = write!(self.sql, "{column} = ?{}", self.params.len());
        }
    }

    /// Adds `column LIKE ?` matching `value` as a literal substring (wildcards
    /// escaped), or nothing when `value` is `None`.
    pub(crate) fn like(&mut self, column: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.params
                .push(Value::Text(format!("%{}%", escape_like(value))));
            self.separator();
            let _ = write!(self.sql, "{column} LIKE ?{} ESCAPE '\\'", self.params.len());
        }
    }

    /// Adds `column LIKE ?` matching `value` as a literal prefix (wildcards
    /// escaped), or nothing when `value` is `None`.
    pub(crate) fn prefix(&mut self, column: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.params
                .push(Value::Text(format!("{}%", escape_like(value))));
            self.separator();
            let _ = write!(self.sql, "{column} LIKE ?{} ESCAPE '\\'", self.params.len());
        }
    }

    /// Adds `column = ?` bound to the integer `value`, or nothing when `None`.
    pub(crate) fn eq_int(&mut self, column: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.params.push(Value::Integer(value));
            self.separator();
            let _ = write!(self.sql, "{column} = ?{}", self.params.len());
        }
    }

    /// Adds `column >= ?` bound to the integer `value`, or nothing when `None`.
    pub(crate) fn gte_int(&mut self, column: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.params.push(Value::Integer(value));
            self.separator();
            let _ = write!(self.sql, "{column} >= ?{}", self.params.len());
        }
    }

    /// Adds `column <= ?` bound to the integer `value`, or nothing when `None`.
    pub(crate) fn lte_int(&mut self, column: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.params.push(Value::Integer(value));
            self.separator();
            let _ = write!(self.sql, "{column} <= ?{}", self.params.len());
        }
    }

    /// Adds `json_extract(fields, '$.key') = ?` bound to `value`, or nothing
    /// when `value` is `None`. `key` must be a fixed identifier, never user
    /// input, since it is interpolated into SQL.
    pub(crate) fn json_eq(&mut self, key: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.params.push(Value::Text(value.to_owned()));
            self.separator();
            let _ = write!(
                self.sql,
                "json_extract(fields, '$.{key}') = ?{}",
                self.params.len()
            );
        }
    }

    /// Adds a condition with one bind param. `sql` receives the 1-based
    /// placeholder number and must produce the SQL fragment. The param is
    /// pushed after the fragment is written so the placeholder number is
    /// correct.
    pub(crate) fn param(&mut self, value: Value, sql: impl FnOnce(usize) -> String) {
        self.separator();
        let placeholder = self.params.len() + 1;
        let _ = write!(self.sql, "{}", sql(placeholder));
        self.params.push(value);
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

/// Drives one paginated query: resolves the limit, base order, and travel
/// direction, exposes the order to query in, and turns a fetched batch into a
/// [`Page`].
pub(crate) struct Paginator {
    limit: u32,
    cursor: Option<Cursor>,
    step: Step,
    base_order: Order,
}

impl Paginator {
    pub(crate) fn new(query: &ListQuery, default_order: Order) -> Result<Self> {
        let cursor = match &query.cursor {
            Some(encoded) => Some(Cursor::decode(encoded)?),
            None => None,
        };
        let step = cursor.as_ref().map_or(Step::After, |cursor| cursor.step);
        Ok(Self {
            limit: query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
            cursor,
            step,
            base_order: query.order.unwrap_or(default_order),
        })
    }

    /// The decoded cursor position, if any.
    pub(crate) fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }

    /// The order to run the query in. Backward paging queries in the flipped
    /// order, then [`Paginator::finish`] reverses the rows back to base order.
    pub(crate) fn query_order(&self) -> Order {
        match self.step {
            Step::After => self.base_order,
            Step::Before => self.base_order.flip(),
        }
    }

    /// One more than the page size, so an extra row means another page exists.
    pub(crate) fn fetch_limit(&self) -> u32 {
        self.limit + 1
    }

    /// Trims `rows` to the page size, restores base order, and derives the
    /// neighbouring cursors. `key_of` reads a row's sort value and id.
    pub(crate) fn finish<T>(
        &self,
        mut rows: Vec<T>,
        key_of: impl Fn(&T) -> (CursorValue, i64),
    ) -> Page<T> {
        let has_extra = rows.len() as u32 > self.limit;
        if has_extra {
            rows.truncate(self.limit as usize);
        }
        if matches!(self.step, Step::Before) {
            rows.reverse();
        }

        let cursor_at = |row: Option<&T>, step: Step| {
            row.map(|row| {
                let (value, id) = key_of(row);
                Cursor::encode(value, id, step)
            })
        };
        let first = rows.first();
        let last = rows.last();

        let (next_cursor, prev_cursor) = match self.step {
            // Forward: more ahead iff we fetched an extra; a previous page
            // exists iff we arrived here from a cursor.
            Step::After => (
                if has_extra { cursor_at(last, Step::After) } else { None },
                if self.cursor.is_some() {
                    cursor_at(first, Step::Before)
                } else {
                    None
                },
            ),
            // Backward: we came from ahead, so a next page always exists; more
            // behind iff we fetched an extra.
            Step::Before => (
                cursor_at(last, Step::After),
                if has_extra {
                    cursor_at(first, Step::Before)
                } else {
                    None
                },
            ),
        };

        Page {
            items: rows,
            next_cursor,
            prev_cursor,
        }
    }
}

/// Escapes the LIKE wildcards `%` and `_` (and the escape char itself) so a
/// user-supplied substring matches literally. Pair with `ESCAPE '\'` in the SQL.
fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).expect("nibble"));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).expect("nibble"));
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().as_chunks::<2>().0 {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrips_int_and_text() {
        for step in [Step::After, Step::Before] {
            let encoded = Cursor::encode(CursorValue::Int(-42), 7, step);
            let decoded = Cursor::decode(&encoded).unwrap();
            assert_eq!(decoded.value, CursorValue::Int(-42));
            assert_eq!(decoded.id, 7);
            assert_eq!(decoded.step, step);

            let encoded = Cursor::encode(CursorValue::Text("tool/avrdude".to_owned()), 3, step);
            let decoded = Cursor::decode(&encoded).unwrap();
            assert_eq!(decoded.value, CursorValue::Text("tool/avrdude".to_owned()));
            assert_eq!(decoded.step, step);
        }
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(Cursor::decode("zz").is_err());
        assert!(Cursor::decode("00").is_err());
    }

    #[test]
    fn first_page_has_next_but_no_prev() {
        let query = ListQuery {
            limit: Some(2),
            ..Default::default()
        };
        let paginator = Paginator::new(&query, Order::Asc).unwrap();
        let page = paginator.finish(vec![1i64, 2, 3], |n| (CursorValue::Int(*n), *n));
        assert_eq!(page.items, vec![1, 2]);
        assert!(page.next_cursor.is_some());
        assert!(page.prev_cursor.is_none());
    }

    #[test]
    fn last_page_has_prev_but_no_next() {
        // Arrived via a forward cursor, fewer rows than the limit.
        let forward = Cursor::encode(CursorValue::Int(2), 2, Step::After);
        let query = ListQuery {
            limit: Some(2),
            cursor: Some(forward),
            ..Default::default()
        };
        let paginator = Paginator::new(&query, Order::Asc).unwrap();
        let page = paginator.finish(vec![3i64], |n| (CursorValue::Int(*n), *n));
        assert_eq!(page.items, vec![3]);
        assert!(page.next_cursor.is_none());
        assert!(page.prev_cursor.is_some());
    }

    #[test]
    fn backward_page_reverses_and_offers_next() {
        let backward = Cursor::encode(CursorValue::Int(4), 4, Step::Before);
        let query = ListQuery {
            limit: Some(2),
            cursor: Some(backward),
            ..Default::default()
        };
        let paginator = Paginator::new(&query, Order::Asc).unwrap();
        // Rows fetched in flipped (desc) order; finish reverses to base order.
        let page = paginator.finish(vec![3i64, 2, 1], |n| (CursorValue::Int(*n), *n));
        assert_eq!(page.items, vec![2, 3]);
        assert!(page.next_cursor.is_some());
        assert!(page.prev_cursor.is_some());
    }
}
