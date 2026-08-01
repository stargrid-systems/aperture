//! The generic response-page type.
//!
//! `option_if_let_else` is allowed module-locally because utoipa's `ToSchema`
//! derive triggers a clippy false positive on the generic `Vec<T>` field that
//! cannot be suppressed at the item level.

#![expect(clippy::option_if_let_else)]

use aperture_artifacts::Page as StoragePage;
use serde::Serialize;
use utoipa::ToSchema;

/// A page of results plus the cursors for the neighbouring pages.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Page<T> {
    /// The rows in this page.
    pub items: Vec<T>,
    /// Cursor to pass as `?cursor=` for the next page. Null at the end.
    pub next_cursor: Option<String>,
    /// Cursor to pass as `?cursor=` for the previous page. Null at the start.
    pub prev_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Maps a storage page into a response page.
    pub fn from_storage<S>(page: StoragePage<S>, map: impl Fn(S) -> T) -> Self {
        Self {
            next_cursor: page.next_cursor,
            prev_cursor: page.prev_cursor,
            items: page.items.into_iter().map(map).collect(),
        }
    }
}
