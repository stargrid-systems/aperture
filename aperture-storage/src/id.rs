//! Database row ID newtype.

use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use serde::{Deserialize, Serialize, de};

/// Primary key of a row in the database (SQLite rowid).
///
/// Wraps an [`i64`] so that parsing and display are centralized on the type
/// itself rather than scattered as free functions. Serializes as a string to
/// keep the API representation opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, utoipa::ToSchema)]
#[schema(value_type = String)]
pub struct DbId(i64);

impl DbId {
    pub const fn get(self) -> i64 {
        self.0
    }

    pub const fn from_i64(value: i64) -> Self {
        Self(value)
    }
}

impl From<i64> for DbId {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<DbId> for i64 {
    fn from(id: DbId) -> Self {
        id.0
    }
}

impl fmt::Display for DbId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for DbId {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl Serialize for DbId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DbId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = DbId;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a database identifier string")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.parse().map_err(de::Error::custom)
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}
