//! Small helpers for serde impls on `FromStr` + `Display` newtypes.

use std::fmt::{self, Display};
use std::marker::PhantomData;
use std::str::FromStr;

use serde::Deserializer;
use serde::de::{self, Visitor};

/// Deserializes a `T: FromStr` from a string, borrowing from the input when
/// the deserializer can offer a borrowed `&str`.
///
/// Serializers that hand us an owned `String` pay one allocation (the input
/// itself). With [`String::deserialize`] serde always asks for an owned string
/// even when the format has a borrowed one available. This visitor accepts
/// `visit_borrowed_str` so a JSON decoder reading from `&[u8]` can skip that
/// extra copy.
pub(super) fn deserialize_from_str<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    struct FromStrVisitor<T>(PhantomData<T>);

    impl<'de, T> Visitor<'de> for FromStrVisitor<T>
    where
        T: FromStr,
        T::Err: Display,
    {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
            T::from_str(v).map_err(E::custom)
        }

        fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<T, E> {
            T::from_str(v).map_err(E::custom)
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<T, E> {
            T::from_str(&v).map_err(E::custom)
        }
    }

    deserializer.deserialize_str(FromStrVisitor(PhantomData))
}
