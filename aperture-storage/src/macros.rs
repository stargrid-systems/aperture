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

/// Generates a typed ID newtype wrapping [`DbId`]($crate::id::DbId) with all
/// standard trait impls: `Display`, `FromStr`, `Serialize`, `Deserialize`,
/// `ToSchema`, `From<i64>`, `get`, `from_i64`, `ToSql`, and `FromSql`.
///
/// Each entity gets its own type so IDs cannot be accidentally mixed up.
macro_rules! db_id {
    ($(#[$meta:meta])* $vis:vis struct $name:ident;) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, utoipa::ToSchema)]
        #[schema(value_type = String)]
        $vis struct $name($crate::id::DbId);

        impl $name {
            pub const fn get(self) -> i64 {
                self.0.get()
            }

            pub const fn from_i64(value: i64) -> Self {
                Self($crate::id::DbId::from_i64(value))
            }
        }

        impl From<i64> for $name {
            fn from(value: i64) -> Self {
                Self($crate::id::DbId::from(value))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = std::num::ParseIntError;
            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                s.parse::<i64>().map(|v| Self($crate::id::DbId::from(v)))
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                self.0.serialize(serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                $crate::id::DbId::deserialize(deserializer).map(Self)
            }
        }

        impl $crate::sql::ToSql for $name {
            fn to_sql(&self) -> turso::Value {
                turso::Value::Integer(self.get())
            }
        }

        impl $crate::sql::FromSql for $name {
            fn from_sql(value: turso::Value, idx: usize) -> $crate::error::Result<Self> {
                <i64 as $crate::sql::FromSql>::from_sql(value, idx).map(Self::from)
            }
        }
    };
}

pub(crate) use db_id;
pub(crate) use sql;
