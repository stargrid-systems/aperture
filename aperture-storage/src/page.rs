//! Cursor-based (keyset) pagination over catalog listings.
//!
//! A listing orders rows by one column plus the unique `id` as a tiebreaker.
//! The cursor carries the last row's sort value and id, so the next page
//! resumes right after it. This stays correct even when rows are inserted
//! between page fetches, as long as the sort field and direction do not change.

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

/// A page of results plus an opaque cursor for the next page, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    /// The rows in this page.
    pub items: Vec<T>,
    /// Cursor to pass back to fetch the following page. `None` at the end.
    pub next_cursor: Option<String>,
}

/// How much to return and where to resume from.
#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    /// Maximum rows to return. Clamped to `1..=200`. Defaults to 50.
    pub limit: Option<u32>,
    /// Opaque cursor from a previous page's `next_cursor`.
    pub cursor: Option<String>,
    /// Sort direction. Each listing applies its own default.
    pub order: Option<Order>,
}

impl ListQuery {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }

    fn cursor(&self) -> Result<Option<Cursor>> {
        match &self.cursor {
            Some(encoded) => Cursor::decode(encoded).map(Some),
            None => Ok(None),
        }
    }
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

/// A decoded keyset position: the last row's sort value and its unique id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cursor {
    value: CursorValue,
    id: i64,
}

impl Cursor {
    fn encode(&self) -> String {
        let mut buf = Vec::new();
        match &self.value {
            CursorValue::Int(int) => {
                buf.push(0);
                buf.extend_from_slice(&self.id.to_be_bytes());
                buf.extend_from_slice(&int.to_be_bytes());
            }
            CursorValue::Text(text) => {
                buf.push(1);
                buf.extend_from_slice(&self.id.to_be_bytes());
                buf.extend_from_slice(text.as_bytes());
            }
        }
        to_hex(&buf)
    }

    fn decode(encoded: &str) -> Result<Self> {
        let invalid = || StorageError::Decode(format!("invalid cursor {encoded:?}"));
        let buf = from_hex(encoded).ok_or_else(invalid)?;
        if buf.len() < 9 {
            return Err(invalid());
        }
        let id = i64::from_be_bytes(buf[1..9].try_into().expect("8 bytes"));
        let value = match buf[0] {
            0 if buf.len() == 17 => {
                CursorValue::Int(i64::from_be_bytes(buf[9..17].try_into().expect("8 bytes")))
            }
            1 => CursorValue::Text(String::from_utf8(buf[9..].to_vec()).map_err(|_| invalid())?),
            _ => return Err(invalid()),
        };
        Ok(Self { value, id })
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

    /// The keyset `WHERE` condition that resumes after `cursor`, using bind
    /// params starting at `first_param`. Empty (and no params) without a cursor.
    pub(crate) fn condition(
        &self,
        cursor: Option<&Cursor>,
        first_param: usize,
    ) -> (String, Vec<Value>) {
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
            (format!("{col} {op} ?{first_param}"), vec![cursor.value.to_db()])
        }
    }
}

/// Holds a query's limit and decoded cursor, and turns a fetched batch into a
/// [`Page`]. Fetch `limit() + 1` rows so a full batch signals more to come.
pub(crate) struct Paginator {
    limit: u32,
    cursor: Option<Cursor>,
}

impl Paginator {
    pub(crate) fn new(query: &ListQuery) -> Result<Self> {
        Ok(Self {
            limit: query.limit(),
            cursor: query.cursor()?,
        })
    }

    pub(crate) fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }

    /// One more than the page size, so an extra row means another page exists.
    pub(crate) fn fetch_limit(&self) -> u32 {
        self.limit + 1
    }

    /// Trims `items` to the page size and returns the cursor for the next page,
    /// derived from the last kept row by `key_of`.
    pub(crate) fn finish<T>(
        &self,
        mut items: Vec<T>,
        key_of: impl Fn(&T) -> (CursorValue, i64),
    ) -> Page<T> {
        let next_cursor = if items.len() as u32 > self.limit {
            items.truncate(self.limit as usize);
            items.last().map(|last| {
                let (value, id) = key_of(last);
                Cursor { value, id }.encode()
            })
        } else {
            None
        };
        Page { items, next_cursor }
    }
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
    for pair in text.as_bytes().chunks_exact(2) {
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
        let int = Cursor {
            value: CursorValue::Int(-42),
            id: 7,
        };
        assert_eq!(Cursor::decode(&int.encode()).unwrap(), int);

        let text = Cursor {
            value: CursorValue::Text("tool/avrdude".to_owned()),
            id: 3,
        };
        assert_eq!(Cursor::decode(&text.encode()).unwrap(), text);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(Cursor::decode("zz").is_err());
        assert!(Cursor::decode("00").is_err());
    }

    #[test]
    fn finish_sets_cursor_only_when_over_limit() {
        let query = ListQuery {
            limit: Some(2),
            ..Default::default()
        };
        let paginator = Paginator::new(&query).unwrap();

        let full = paginator.finish(vec![1, 2, 3], |n| (CursorValue::Int(*n), *n));
        assert_eq!(full.items, vec![1, 2]);
        assert!(full.next_cursor.is_some());

        let partial = paginator.finish(vec![1], |n| (CursorValue::Int(*n), *n));
        assert_eq!(partial.items, vec![1]);
        assert!(partial.next_cursor.is_none());
    }
}
