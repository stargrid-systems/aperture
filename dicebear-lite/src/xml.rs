//! XML entity escaping, matching `Utils/Xml.escape`, streamed without
//! allocating.

use core::fmt;

/// Writes `value` with the five significant XML characters escaped.
pub fn escape_into<W: fmt::Write>(f: &mut W, value: &str) -> fmt::Result {
    for ch in value.chars() {
        match ch {
            '&' => f.write_str("&amp;")?,
            '\'' => f.write_str("&apos;")?,
            '"' => f.write_str("&quot;")?,
            '<' => f.write_str("&lt;")?,
            '>' => f.write_str("&gt;")?,
            other => f.write_char(other)?,
        }
    }
    Ok(())
}
