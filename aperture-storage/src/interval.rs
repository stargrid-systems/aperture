//! A strictly-positive, fixed-unit time interval.
//!
//! [`Interval`] wraps a [`jiff::Span`] that is guaranteed to be positive and
//! representable as a whole number of milliseconds. Calendar units (years,
//! months, weeks, days) are rejected, because the interval is persisted as
//! milliseconds and those units have no fixed length. On the wire an interval
//! is an ISO 8601 duration such as `PT5M`; jiff also accepts its friendlier
//! form (`5m`) on input.

use std::fmt;
use std::result::Result as StdResult;

use jiff::{SignedDuration, Span};
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use turso::Value;

use crate::error::{Result, StorageError};
use crate::sql::{FromSql, ToSql};

/// A strictly-positive span of time, stored as a whole number of milliseconds.
///
/// Construct with [`Interval::new`] (from a [`Span`]) or
/// [`Interval::from_millis`]. Both reject zero, negative, and calendar-based
/// (variable-length) spans.
#[derive(Debug, Clone)]
pub struct Interval {
    span: Span,
    millis: i64,
}

/// Errors from constructing an [`Interval`].
#[derive(Debug, thiserror::Error)]
pub enum InvalidInterval {
    /// The span was zero or negative.
    #[error("interval must be strictly positive")]
    NotPositive,
    /// The span used variable-length units (days, months, ...) that have no
    /// single millisecond length.
    #[error("interval must use fixed units no larger than hours; calendar units are not supported")]
    NotFixed,
    /// The span is outside the supported range.
    #[error("interval is out of range")]
    OutOfRange,
}

impl Interval {
    /// Creates an interval from a [`Span`], validating that it is strictly
    /// positive and expressible as a whole number of milliseconds.
    pub fn new(span: Span) -> StdResult<Self, InvalidInterval> {
        if !span.is_positive() {
            return Err(InvalidInterval::NotPositive);
        }
        // try_from fails when the span uses variable-length units (days or
        // larger), which have no single millisecond length.
        let millis = SignedDuration::try_from(span)
            .map_err(|_| InvalidInterval::NotFixed)?
            .as_millis();
        let millis = i64::try_from(millis).map_err(|_| InvalidInterval::OutOfRange)?;
        Ok(Self { span, millis })
    }

    /// Creates an interval from a positive number of milliseconds.
    pub fn from_millis(millis: i64) -> StdResult<Self, InvalidInterval> {
        if millis <= 0 {
            return Err(InvalidInterval::NotPositive);
        }
        let span = Span::new()
            .try_milliseconds(millis)
            .map_err(|_| InvalidInterval::OutOfRange)?;
        Ok(Self { span, millis })
    }

    /// The interval as a [`Span`], for datetime arithmetic.
    pub fn as_span(&self) -> &Span {
        &self.span
    }

    /// The interval as a whole number of milliseconds.
    pub fn as_millis(&self) -> i64 {
        self.millis
    }
}

impl PartialEq for Interval {
    fn eq(&self, other: &Self) -> bool {
        self.millis == other.millis
    }
}

impl Eq for Interval {}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.span.fmt(f)
    }
}

impl Serialize for Interval {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.span.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Interval {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let span = Span::deserialize(deserializer)?;
        Interval::new(span).map_err(DeError::custom)
    }
}

impl ToSql for Interval {
    fn to_sql(&self) -> Value {
        Value::Integer(self.millis)
    }
}

impl FromSql for Interval {
    fn from_sql(value: Value, idx: usize) -> Result<Self> {
        let millis = i64::from_sql(value, idx)?;
        Interval::from_millis(millis).map_err(|err| StorageError::InvalidInterval {
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
            Interval::new(Span::new()),
            Err(InvalidInterval::NotPositive)
        ));
        assert!(matches!(
            Interval::from_millis(0),
            Err(InvalidInterval::NotPositive)
        ));
        assert!(matches!(
            Interval::from_millis(-5),
            Err(InvalidInterval::NotPositive)
        ));
    }

    #[test]
    fn rejects_calendar_units() {
        // 1 month has no fixed millisecond length.
        let span = Span::new().try_months(1).unwrap();
        assert!(matches!(
            Interval::new(span),
            Err(InvalidInterval::NotFixed)
        ));
    }

    #[test]
    fn round_trips_through_millis() {
        let interval = Interval::from_millis(90_000).unwrap();
        assert_eq!(interval.as_millis(), 90_000);
        // 90 seconds == 1m30s.
        let again = Interval::new(*interval.as_span()).unwrap();
        assert_eq!(again, interval);
    }

    #[test]
    fn serde_is_iso_8601() {
        let interval = Interval::from_millis(300_000).unwrap(); // 5 minutes
        // jiff balances 300_000 ms up to seconds for display.
        let json = serde_json::to_string(&interval).unwrap();
        assert_eq!(json, "\"PT300S\"");
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
