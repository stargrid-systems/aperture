//! A strictly-positive time interval.
//!
//! [`Interval`] wraps a [`jiff::SignedDuration`] that is guaranteed to be
//! positive and representable as a whole number of microseconds. On the wire
//! an interval is an ISO 8601 duration such as `PT5M`; jiff also accepts its
//! friendlier form (`5m`) on input.

use std::fmt;
use std::result::Result as StdResult;

use jiff::SignedDuration;
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use turso::Value;

use crate::error::{Result, StorageError};
use crate::sql::{FromSql, ToSql};

/// A strictly-positive span of time, stored as a whole number of microseconds.
///
/// Construct with [`Interval::new`] (from a [`SignedDuration`]) or
/// [`Interval::from_micros`]. Both reject zero and negative durations, and
/// require the duration to fit in `i64` microseconds.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schema", schema(value_type = String, example = "PT5M"))]
pub struct Interval(SignedDuration);

/// Errors from constructing an [`Interval`].
#[derive(Debug, thiserror::Error)]
pub enum InvalidInterval {
    /// The duration was zero or negative.
    #[error("interval must be strictly positive")]
    NotPositive,
    /// The duration is outside the supported range.
    #[error("interval is out of range")]
    OutOfRange,
}

impl Interval {
    /// Creates an interval from a [`SignedDuration`], validating that it is
    /// strictly positive and expressible as a whole number of microseconds.
    pub fn new(duration: SignedDuration) -> StdResult<Self, InvalidInterval> {
        if !duration.is_positive() {
            return Err(InvalidInterval::NotPositive);
        }
        i64::try_from(duration.as_micros()).map_err(|_| InvalidInterval::OutOfRange)?;
        Ok(Self(duration))
    }

    /// Creates an interval from a positive number of microseconds.
    pub fn from_micros(micros: i64) -> StdResult<Self, InvalidInterval> {
        if micros <= 0 {
            return Err(InvalidInterval::NotPositive);
        }
        Ok(Self(SignedDuration::from_micros(micros)))
    }

    /// The interval as a [`SignedDuration`], for datetime arithmetic.
    pub fn as_signed_duration(&self) -> &SignedDuration {
        &self.0
    }

    /// The interval as a whole number of microseconds.
    pub fn as_micros(&self) -> i64 {
        i64::try_from(self.0.as_micros()).expect("validated at construction")
    }
}

impl PartialEq for Interval {
    fn eq(&self, other: &Self) -> bool {
        self.as_micros() == other.as_micros()
    }
}

impl Eq for Interval {}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Interval {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Interval {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let duration = SignedDuration::deserialize(deserializer)?;
        Interval::new(duration).map_err(DeError::custom)
    }
}

impl ToSql for Interval {
    fn to_sql(&self) -> Value {
        Value::Integer(self.as_micros())
    }
}

impl FromSql for Interval {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        let micros = i64::from_sql(value, idx)?;
        Interval::from_micros(micros).map_err(|err| StorageError::InvalidInterval {
            error: err.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_and_negative() {
        assert!(matches!(
            Interval::new(SignedDuration::ZERO),
            Err(InvalidInterval::NotPositive)
        ));
        assert!(matches!(
            Interval::from_micros(0),
            Err(InvalidInterval::NotPositive)
        ));
        assert!(matches!(
            Interval::from_micros(-5),
            Err(InvalidInterval::NotPositive)
        ));
    }

    #[test]
    fn round_trips_through_micros() {
        let interval = Interval::from_micros(90_000_000).unwrap();
        assert_eq!(interval.as_micros(), 90_000_000);
        // 90 seconds == 1m30s.
        let again = Interval::new(*interval.as_signed_duration()).unwrap();
        assert_eq!(again, interval);
    }

    #[test]
    fn serde_is_iso_8601() {
        let interval = Interval::from_micros(300_000_000).unwrap(); // 5 minutes
        let json = serde_json::to_string(&interval).unwrap();
        assert_eq!(json, "\"PT5M\"");
        // Deserializing the equivalent 5-minute duration yields the same
        // interval, whether written in ISO 8601 or jiff's friendly form.
        assert_eq!(
            serde_json::from_str::<Interval>("\"PT5M\"").unwrap(),
            interval
        );
        assert_eq!(
            serde_json::from_str::<Interval>("\"5m\"").unwrap(),
            interval
        );
        // Round-trips through the serialized form.
        let back: Interval = serde_json::from_str(&json).unwrap();
        assert_eq!(back, interval);
    }
}
