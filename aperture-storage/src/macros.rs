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

/// Generates a typed ID newtype wrapping an [`i64`] with all standard trait
/// impls: `Display`, `FromStr`, `Serialize`, `Deserialize`, `ToSchema`,
/// `From<i64>`, `get`, `ToSql`, and `FromSql`.
///
/// IDs serialize as strings (see the `Serialize` impl) so the API
/// representation stays opaque, while storage uses the raw integer.
///
/// Each entity gets its own type so IDs cannot be accidentally mixed up.
macro_rules! db_id {
    ($(#[$meta:meta])* $vis:vis struct $name:ident;) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, utoipa::ToSchema)]
        #[schema(value_type = String)]
        $vis struct $name(i64);

        impl $name {
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl From<i64> for $name {
            fn from(value: i64) -> Self {
                Self(value)
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
                s.parse().map(Self)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.collect_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct Visitor;

                impl<'de> serde::de::Visitor<'de> for Visitor {
                    type Value = $name;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("a database identifier string")
                    }

                    fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        v.parse().map_err(serde::de::Error::custom)
                    }
                }

                deserializer.deserialize_str(Visitor)
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
