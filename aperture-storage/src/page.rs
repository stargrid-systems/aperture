//! Cursor-based (keyset) pagination over catalog listings.
//!
//! A listing orders rows by one column plus the unique `id` as a tiebreaker.
//! The cursor carries the last (or first) row's sort value, its id, and the
//! direction to travel. So a single `cursor` value pages either forward or
//! backward, and the caller never needs a separate direction flag. This stays
//! correct even when rows are inserted between page fetches, as long as the
//! sort field and direction do not change.
//!
//! Each cursor also stamps the tag of the listing that issued it, so a cursor
//! replayed against a different listing is rejected with an error instead of
//! silently skipping or dropping rows.

use aperture_runtime::RegistryQuery;
pub use aperture_runtime::{Order, RegistryQuery as ListQuery};
use turso::Value;

use crate::error::{Result, StorageError};

/// The SQL keyword for a sort direction.
const fn keyword(order: Order) -> &'static str {
    match order {
        Order::Asc => "ASC",
        Order::Desc => "DESC",
    }
}

/// The keyset comparison for travel in `order` direction.
const fn comparison(order: Order) -> &'static str {
    match order {
        Order::Asc => ">",
        Order::Desc => "<",
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

/// The sort key value carried by a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorValue {
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
pub struct Cursor {
    value: CursorValue,
    id: i64,
    step: Step,
}

/// The listing a cursor belongs to. Every paginated listing stamps its tag
/// into the cursors it issues, so cursors cannot be replayed across listings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    LogEvents,
    LogTargets,
    LogSpans,
    LogBoots,
    Tasks,
    TaskSchedules,
    Users,
    ArtifactKeys,
    ArtifactVersions,
    ApiKeys,
}

impl Listing {
    /// The flag tag for this listing. Always nonzero, so untagged cursors
    /// never match any listing.
    // Tag 1 is reserved for the events listing a sibling branch adds.
    const fn tag(self) -> u8 {
        match self {
            Self::LogEvents => 2,
            Self::LogTargets => 3,
            Self::LogSpans => 4,
            Self::LogBoots => 5,
            Self::Tasks => 6,
            Self::TaskSchedules => 7,
            Self::Users => 8,
            Self::ArtifactKeys => 9,
            Self::ArtifactVersions => 10,
            Self::ApiKeys => 11,
        }
    }
}

impl Cursor {
    /// The sort value carried by this cursor.
    pub const fn value(&self) -> &CursorValue {
        &self.value
    }

    fn encode(value: CursorValue, id: i64, step: Step, listing: Listing) -> String {
        let mut flags = listing.tag() << 3;
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

    fn decode(encoded: &str, listing: Listing) -> Result<Self> {
        let invalid = || StorageError::InvalidCursor(encoded.to_owned());
        let buf = from_hex(encoded).ok_or_else(invalid)?;
        if buf.len() < 9 {
            return Err(invalid());
        }
        let flags = buf[0];
        if flags >> 3 != listing.tag() {
            return Err(invalid());
        }
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
pub struct Keyset {
    column: &'static str,
    order: Order,
    tiebreak: bool,
}

impl Keyset {
    /// Sorts by `column` (a real column unique per row) with no extra
    /// tiebreaker.
    pub(crate) const fn unique(column: &'static str, order: Order) -> Self {
        Self {
            column,
            order,
            tiebreak: false,
        }
    }

    /// Sorts by `column`, breaking ties on the unique `id` column.
    pub(crate) const fn with_id(column: &'static str, order: Order) -> Self {
        Self {
            column,
            order,
            tiebreak: true,
        }
    }

    /// The `ORDER BY` body (without the keyword).
    pub(crate) fn order_by(&self) -> String {
        let dir = keyword(self.order);
        if self.tiebreak {
            format!("{} {dir}, id {dir}", self.column)
        } else {
            format!("{} {dir}", self.column)
        }
    }

    /// The keyset `WHERE` condition that resumes from `cursor`, using bind
    /// params starting at `first_param`. Empty (and no params) without a
    /// cursor.
    pub(crate) fn condition(
        &self,
        cursor: Option<&Cursor>,
        first_param: usize,
    ) -> (String, Vec<Value>) {
        let Some(cursor) = cursor else {
            return (String::new(), Vec::new());
        };
        let col = self.column;
        let op = comparison(self.order);
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

/// Drives one paginated query: resolves the limit, base order, and travel
/// direction, exposes the order to query in, and turns a fetched batch into a
/// [`Page`].
#[derive(Debug)]
pub struct Paginator {
    limit: u32,
    cursor: Option<Cursor>,
    step: Step,
    base_order: Order,
    listing: Listing,
}

impl Paginator {
    /// Creates a paginator for `listing` from query parameters.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::InvalidCursor` if `query.cursor` is not a valid
    /// cursor string issued by `listing`.
    pub fn new(query: &ListQuery, default_order: Order, listing: Listing) -> Result<Self> {
        let cursor = match &query.cursor {
            Some(encoded) => Some(Cursor::decode(encoded, listing)?),
            None => None,
        };
        let step = cursor.as_ref().map_or(Step::After, |cursor| cursor.step);
        Ok(Self {
            limit: query
                .limit
                .unwrap_or(RegistryQuery::DEFAULT_LIMIT)
                .clamp(1, RegistryQuery::MAX_LIMIT),
            cursor,
            step,
            base_order: query.order.unwrap_or(default_order),
            listing,
        })
    }

    /// The decoded cursor position, if any.
    pub const fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }

    /// The order to run the query in. Backward paging queries in the flipped
    /// order, then [`Paginator::finish`] reverses the rows back to base order.
    pub const fn query_order(&self) -> Order {
        match self.step {
            Step::After => self.base_order,
            Step::Before => self.base_order.flip(),
        }
    }

    /// One more than the page size, so an extra row means another page exists.
    pub const fn fetch_limit(&self) -> u32 {
        self.limit + 1
    }

    /// Trims `rows` to the page size, restores base order, and derives the
    /// neighbouring cursors. `key_of` reads a row's sort value and id.
    pub fn finish<T>(
        &self,
        mut rows: Vec<T>,
        key_of: impl Fn(&T) -> (CursorValue, i64),
    ) -> Page<T> {
        let has_extra = rows.len() > self.limit as usize;
        if has_extra {
            rows.truncate(self.limit as usize);
        }
        if matches!(self.step, Step::Before) {
            rows.reverse();
        }

        let cursor_at = |row: Option<&T>, step: Step| {
            row.map(|row| {
                let (value, id) = key_of(row);
                Cursor::encode(value, id, step, self.listing)
            })
        };
        let first = rows.first();
        let last = rows.last();

        let (next_cursor, prev_cursor) = match self.step {
            // Forward: more ahead iff we fetched an extra. A previous page
            // exists iff we arrived here from a cursor.
            Step::After => (
                if has_extra {
                    cursor_at(last, Step::After)
                } else {
                    None
                },
                if self.cursor.is_some() {
                    cursor_at(first, Step::Before)
                } else {
                    None
                },
            ),
            // Backward: we came from ahead, so a next page always exists. More
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

fn to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    hex::decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrips_int_and_text() {
        for step in [Step::After, Step::Before] {
            let encoded = Cursor::encode(CursorValue::Int(-42), 7, step, Listing::LogEvents);
            let decoded = Cursor::decode(&encoded, Listing::LogEvents).unwrap();
            assert_eq!(decoded.value, CursorValue::Int(-42));
            assert_eq!(decoded.id, 7);
            assert_eq!(decoded.step, step);

            let encoded = Cursor::encode(
                CursorValue::Text("tool/avrdude".to_owned()),
                3,
                step,
                Listing::LogEvents,
            );
            let decoded = Cursor::decode(&encoded, Listing::LogEvents).unwrap();
            assert_eq!(decoded.value, CursorValue::Text("tool/avrdude".to_owned()));
            assert_eq!(decoded.step, step);
        }
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(Cursor::decode("zz", Listing::LogEvents).is_err());
        assert!(Cursor::decode("00", Listing::LogEvents).is_err());
    }

    #[test]
    fn rejects_cursor_from_another_listing() {
        let encoded = Cursor::encode(CursorValue::Int(1), 1, Step::After, Listing::LogEvents);
        let query = ListQuery {
            cursor: Some(encoded),
            ..Default::default()
        };
        let err = Paginator::new(&query, Order::Desc, Listing::LogTargets).unwrap_err();
        assert!(matches!(err, StorageError::InvalidCursor(_)));
    }

    #[test]
    fn rejects_untagged_cursor() {
        // Pre-tag cursors left the upper five flag bits at zero.
        let encoded = Cursor::encode(CursorValue::Int(1), 1, Step::After, Listing::LogEvents);
        let mut buf = from_hex(&encoded).unwrap();
        buf[0] &= 0b0000_0111;
        let untagged = to_hex(&buf);
        let query = ListQuery {
            cursor: Some(untagged),
            ..Default::default()
        };
        let err = Paginator::new(&query, Order::Desc, Listing::LogEvents).unwrap_err();
        assert!(matches!(err, StorageError::InvalidCursor(_)));
    }

    #[test]
    fn first_page_has_next_but_no_prev() {
        let query = ListQuery {
            limit: Some(2),
            ..Default::default()
        };
        let paginator = Paginator::new(&query, Order::Asc, Listing::LogEvents).unwrap();
        let page = paginator.finish(vec![1i64, 2, 3], |n| (CursorValue::Int(*n), *n));
        assert_eq!(page.items, vec![1, 2]);
        assert!(page.next_cursor.is_some());
        assert!(page.prev_cursor.is_none());
    }

    #[test]
    fn last_page_has_prev_but_no_next() {
        // Arrived via a forward cursor, fewer rows than the limit.
        let forward = Cursor::encode(CursorValue::Int(2), 2, Step::After, Listing::LogEvents);
        let query = ListQuery {
            limit: Some(2),
            cursor: Some(forward),
            ..Default::default()
        };
        let paginator = Paginator::new(&query, Order::Asc, Listing::LogEvents).unwrap();
        let page = paginator.finish(vec![3i64], |n| (CursorValue::Int(*n), *n));
        assert_eq!(page.items, vec![3]);
        assert!(page.next_cursor.is_none());
        assert!(page.prev_cursor.is_some());
    }

    #[test]
    fn backward_page_reverses_and_offers_next() {
        let backward = Cursor::encode(CursorValue::Int(4), 4, Step::Before, Listing::LogEvents);
        let query = ListQuery {
            limit: Some(2),
            cursor: Some(backward),
            ..Default::default()
        };
        let paginator = Paginator::new(&query, Order::Asc, Listing::LogEvents).unwrap();
        // Rows fetched in flipped (desc) order. Finish reverses to base order.
        let page = paginator.finish(vec![3i64, 2, 1], |n| (CursorValue::Int(*n), *n));
        assert_eq!(page.items, vec![2, 3]);
        assert!(page.next_cursor.is_some());
        assert!(page.prev_cursor.is_some());
    }
}
