//! XML entity escaping, matching `Utils/Xml.escape`.

/// Escapes the five significant XML characters. Everything else (including
/// non-ASCII such as curly quotes) is left untouched.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}
