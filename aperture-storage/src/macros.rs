//! Internal macros.

/// Marks raw SQL tokens so editors highlight them as SQL. Expands to the
/// stringified tokens (a `&'static str`, zero allocation).
///
/// For a query assembled from pieces, wrap it in `format!`, keeping the SQL as
/// raw tokens so it still highlights: `format!(sql!(SELECT {cols} FROM x), cols
/// = COLS)`. Values must still go through bind params (`?1`), never `format!`.
macro_rules! sql {
    ($($query:tt)*) => {
        stringify!($($query)*)
    };
}

pub(crate) use sql;
