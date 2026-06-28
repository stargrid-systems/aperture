//! Internal macros.

/// Marks SQL so editors highlight it, and keeps the query text in one place.
///
/// Two forms:
/// - Raw tokens, `sql!(SELECT ...)`, expand to a `&'static str` via
///   [`stringify`] (zero allocation). Use for fully static queries.
/// - A string-literal template plus arguments, `sql!("SELECT {cols} ...", cols
///   = COLS)`, expand to [`format`]. Use when a query is assembled from pieces.
macro_rules! sql {
    ($fmt:literal, $($args:tt)*) => {
        ::std::format!($fmt, $($args)*)
    };
    ($($query:tt)*) => {
        stringify!($($query)*)
    };
}

pub(crate) use sql;
