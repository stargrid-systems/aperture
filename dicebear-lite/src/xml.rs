//! XML entity escaping, matching `Utils/Xml.escape`.

use core::fmt;

/// A string wrapper that escapes the five significant XML characters when
/// formatted.
pub struct Escaped<'a>(pub &'a str);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in self.0.chars() {
            match ch {
                '&' => f.write_str("&amp;")?,
                '\'' => f.write_str("&apos;")?,
                '"' => f.write_str("&quot;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                other => write!(f, "{other}")?,
            }
        }
        Ok(())
    }
}
