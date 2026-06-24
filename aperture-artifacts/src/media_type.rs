//! Media type newtype.

use std::fmt;

/// A content media type, for example `application/vnd.spectra.tar+gzip`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaType(Box<str>);

impl MediaType {
    /// Returns the media type as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for MediaType {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for MediaType {
    fn from(value: String) -> Self {
        Self(value.into())
    }
}
