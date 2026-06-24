//! Internal macros.

/// Marks raw SQL tokens so editors highlight them as SQL. Expands to the
/// stringified tokens.
macro_rules! sql {
    ($($query:tt)*) => {
        stringify!($($query)*)
    };
}

pub(crate) use sql;
