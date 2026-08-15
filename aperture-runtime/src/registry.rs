//! The definition registry: a keyed map with keyed registration and cursor
//! pagination.

use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::Arc;

/// An entry stored in a [`Registry`].
///
/// Implemented by each domain's erased definition trait so that the registry
/// owns keyed registration ([`Registry::register`]) and pagination
/// ([`Registry::list`]).
pub trait RegistryEntry: Send + Sync + 'static {
    /// The key this entry is registered under.
    fn key(&self) -> &'static str;
}

/// Sort direction for a listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl Order {
    /// The opposite direction.
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

/// How much of a registry to return and where to resume from.
#[derive(Debug, Clone, Default)]
pub struct RegistryQuery {
    /// Maximum entries to return. Clamped to
    /// [`DEFAULT_LIMIT`](RegistryQuery::DEFAULT_LIMIT) and
    /// [`MAX_LIMIT`](RegistryQuery::MAX_LIMIT). Defaults to 50.
    pub limit: Option<u32>,
    /// Opaque cursor from a page's `next_cursor` or `prev_cursor`.
    pub cursor: Option<String>,
    /// Sort direction. Defaults to ascending.
    pub order: Option<Order>,
}

impl RegistryQuery {
    /// The page size used when `limit` is not given.
    pub const DEFAULT_LIMIT: u32 = 50;

    /// The largest page size a query may return.
    pub const MAX_LIMIT: u32 = 200;
}

/// A cursor string was not issued by this registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCursor;

/// A page of registry entries plus the cursors for the neighbouring pages.
pub struct RegistryPage<T: ?Sized> {
    /// The entries in this page, in base order.
    pub items: Vec<Arc<T>>,
    /// Cursor to pass as `?cursor=` for the next page. None at the end.
    pub next_cursor: Option<String>,
    /// Cursor to pass as `?cursor=` for the previous page. None at the start.
    pub prev_cursor: Option<String>,
}

/// Which way to travel from a cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Entries after the cursor, in base order (the next page).
    After,
    /// Entries before the cursor, in base order (the previous page).
    Before,
}

/// A keyset position in a registry listing.
struct Cursor {
    key: String,
    step: Step,
}

impl Cursor {
    /// Encodes the cursor as an opaque hex string.
    fn encode(key: &str, step: Step) -> String {
        let mut buf = vec![u8::from(matches!(step, Step::Before))];
        buf.extend_from_slice(key.as_bytes());
        hex::encode(buf)
    }

    /// Decodes a cursor issued by [`Cursor::encode`].
    ///
    /// Returns `None` for anything else: wrong framing, an unknown flag, or
    /// an empty key.
    fn decode(encoded: &str) -> Option<Self> {
        let buf = hex::decode(encoded).ok()?;
        let (flag, key) = buf.split_first()?;
        let step = match *flag {
            0 => Step::After,
            1 => Step::Before,
            _ => return None,
        };
        if key.is_empty() {
            return None;
        }
        let key = str::from_utf8(key).ok()?.to_owned();
        Some(Self { key, step })
    }
}

/// A registry of definitions keyed by a static string.
///
/// Each domain instantiates this with its own erased trait object type (e.g.
/// `Registry<dyn ErasedTaskDefinition>`). The registry owns duplicate
/// detection and deterministic iteration order.
///
/// Iteration order is deterministic (sorted by key) so that listing endpoints
/// are reproducible across runs.
pub struct Registry<T: ?Sized + RegistryEntry> {
    entries: BTreeMap<&'static str, Arc<T>>,
}

impl<T: ?Sized + RegistryEntry> Registry<T> {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks up the entry for `key`.
    pub fn get(&self, key: &str) -> Option<&Arc<T>> {
        self.entries.get(key)
    }

    /// Iterates over registered keys, in sorted order.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.keys().copied()
    }

    /// Iterates over registered entries, in sorted key order.
    pub fn values(&self) -> impl Iterator<Item = &Arc<T>> + '_ {
        self.entries.values()
    }

    /// The number of registered entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no entries are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Registers `entry` under its own key. The key always comes from the
    /// entry itself, so an entry can never be filed under a key that differs
    /// from the one it advertises.
    ///
    /// # Panics
    ///
    /// Panics if the key is already registered.
    pub fn register(&mut self, entry: Arc<T>) {
        let key = entry.key();
        let prev = self.entries.insert(key, entry);
        assert!(prev.is_none(), "duplicate key {key:?}");
    }

    /// Lists one page of entries, ordered by key.
    ///
    /// Mirrors the storage listing semantics: a single opaque cursor pages
    /// forward or backward, and one extra entry is fetched to detect whether a
    /// neighbouring page exists.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCursor`] if `query.cursor` is not a cursor issued by
    /// this registry.
    pub fn list(&self, query: &RegistryQuery) -> Result<RegistryPage<T>, InvalidCursor> {
        let limit = query
            .limit
            .unwrap_or(RegistryQuery::DEFAULT_LIMIT)
            .clamp(1, RegistryQuery::MAX_LIMIT) as usize;
        let base_order = query.order.unwrap_or(Order::Asc);
        let cursor = query.cursor.as_deref().and_then(Cursor::decode);
        if cursor.is_none() && query.cursor.is_some() {
            return Err(InvalidCursor);
        }
        let step = cursor.as_ref().map_or(Step::After, |cursor| cursor.step);

        // Fetch travelling from the cursor, one extra entry to detect more
        // pages.
        let query_order = match step {
            Step::After => base_order,
            Step::Before => base_order.flip(),
        };
        let mut batch: Vec<&Arc<T>> = match (query_order, cursor.as_ref()) {
            (Order::Asc, None) => self
                .entries
                .range::<str, _>(..)
                .map(|(_, entry)| entry)
                .take(limit + 1)
                .collect(),
            (Order::Asc, Some(cursor)) => self
                .entries
                .range::<str, _>((Excluded(cursor.key.as_str()), Unbounded))
                .map(|(_, entry)| entry)
                .take(limit + 1)
                .collect(),
            (Order::Desc, None) => self
                .entries
                .range::<str, _>(..)
                .rev()
                .map(|(_, entry)| entry)
                .take(limit + 1)
                .collect(),
            (Order::Desc, Some(cursor)) => self
                .entries
                .range::<str, _>((Unbounded, Excluded(cursor.key.as_str())))
                .rev()
                .map(|(_, entry)| entry)
                .take(limit + 1)
                .collect(),
        };

        let has_extra = batch.len() > limit;
        if has_extra {
            batch.truncate(limit);
        }
        if matches!(step, Step::Before) {
            batch.reverse();
        }

        let cursor_at = |entry: Option<&Arc<T>>, step: Step| {
            entry.map(|entry| Cursor::encode(entry.key(), step))
        };
        let (next_cursor, prev_cursor) = match step {
            // Forward: more ahead iff we fetched an extra. A previous page
            // exists iff we arrived here from a cursor.
            Step::After => (
                has_extra
                    .then(|| cursor_at(batch.last().copied(), Step::After))
                    .flatten(),
                cursor
                    .as_ref()
                    .is_some()
                    .then(|| cursor_at(batch.first().copied(), Step::Before))
                    .flatten(),
            ),
            // Backward: we came from ahead, so the next page is offered from
            // the last entry. More behind iff we fetched an extra.
            Step::Before => (
                cursor_at(batch.last().copied(), Step::After),
                has_extra
                    .then(|| cursor_at(batch.first().copied(), Step::Before))
                    .flatten(),
            ),
        };

        Ok(RegistryPage {
            items: batch.into_iter().cloned().collect(),
            next_cursor,
            prev_cursor,
        })
    }
}

impl<T: ?Sized + RegistryEntry> Default for Registry<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeEntry {
        key: &'static str,
    }

    impl RegistryEntry for FakeEntry {
        fn key(&self) -> &'static str {
            self.key
        }
    }

    type FakeRegistry = Registry<dyn RegistryEntry>;

    fn registry(keys: &[&'static str]) -> FakeRegistry {
        let mut registry = FakeRegistry::new();
        for key in keys {
            registry.register(Arc::new(FakeEntry { key }));
        }
        registry
    }

    fn page_keys(page: &RegistryPage<dyn RegistryEntry>) -> Vec<&'static str> {
        page.items.iter().map(|entry| entry.key()).collect()
    }

    #[test]
    fn register_reads_key_from_entry() {
        let registry = registry(&["b", "a"]);
        assert_eq!(registry.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(registry.len(), 2);
        assert!(registry.get("a").is_some());
        assert!(!registry.is_empty());

        assert!(FakeRegistry::new().is_empty());
    }

    #[test]
    #[should_panic(expected = "duplicate key \"a\"")]
    fn register_panics_on_duplicate_key() {
        registry(&["a", "a"]);
    }

    #[test]
    fn pages_forward_and_back() {
        let registry = registry(&["a", "b", "c", "d"]);
        let query = |cursor, limit| RegistryQuery {
            limit: Some(limit),
            cursor,
            order: None,
        };

        // First page.
        let page = registry.list(&query(None, 2)).unwrap();
        assert_eq!(page_keys(&page), ["a", "b"]);
        assert!(page.next_cursor.is_some());
        assert!(page.prev_cursor.is_none());

        // Follow next.
        let page = registry.list(&query(page.next_cursor, 2)).unwrap();
        assert_eq!(page_keys(&page), ["c", "d"]);
        assert!(page.next_cursor.is_none());
        assert!(page.prev_cursor.is_some());

        // Follow prev back to the first page.
        let page = registry.list(&query(page.prev_cursor, 2)).unwrap();
        assert_eq!(page_keys(&page), ["a", "b"]);
    }

    #[test]
    fn pages_in_descending_order() {
        let registry = registry(&["a", "b", "c"]);
        let query = |cursor| RegistryQuery {
            cursor,
            limit: Some(1),
            order: Some(Order::Desc),
        };

        let page = registry.list(&query(None)).unwrap();
        assert_eq!(page_keys(&page), ["c"]);
        let page = registry.list(&query(page.next_cursor)).unwrap();
        assert_eq!(page_keys(&page), ["b"]);
        assert!(page.prev_cursor.is_some());
        let page = registry.list(&query(page.prev_cursor)).unwrap();
        assert_eq!(page_keys(&page), ["c"]);
    }

    #[test]
    fn clamps_the_limit() {
        let registry = registry(&["a", "b"]);
        let page = registry
            .list(&RegistryQuery {
                limit: Some(0),
                ..RegistryQuery::default()
            })
            .unwrap();
        assert_eq!(page_keys(&page), ["a"]);
    }

    #[test]
    fn rejects_foreign_cursors() {
        let registry = registry(&["a"]);
        for encoded in ["not-a-cursor", "00", "02aa", &hex::encode("a")] {
            let result = registry.list(&RegistryQuery {
                cursor: Some(encoded.to_owned()),
                ..RegistryQuery::default()
            });
            let Err(err) = result else {
                panic!("expected cursor {encoded:?} to be rejected");
            };
            assert_eq!(err, InvalidCursor);
        }
    }

    #[test]
    fn empty_backward_page_has_no_cursors() {
        let registry = registry(&["a", "b"]);
        let page = registry
            .list(&RegistryQuery {
                cursor: Some(Cursor::encode("a", Step::Before)),
                ..RegistryQuery::default()
            })
            .unwrap();
        assert!(page_keys(&page).is_empty());
        assert!(page.prev_cursor.is_none());
        assert!(page.next_cursor.is_none());
    }
}
