//! Color helpers. Constellation uses neither contrast sorting nor
//! `notEqualTo` filtering, so only normalization is needed.

/// Normalizes a hex color to a `#`-prefixed lowercase string, expanding
/// 3-/4-digit shorthand. Mirrors `Utils/Color.toHex`.
pub fn to_hex(hex: &str) -> String {
    let h = hex.strip_prefix('#').unwrap_or(hex).to_ascii_lowercase();
    let chars: Vec<char> = h.chars().collect();
    match chars.len() {
        3 => format!(
            "#{}{}{}{}{}{}",
            chars[0], chars[0], chars[1], chars[1], chars[2], chars[2]
        ),
        4 => format!(
            "#{}{}{}{}{}{}{}{}",
            chars[0], chars[0], chars[1], chars[1], chars[2], chars[2], chars[3], chars[3]
        ),
        _ => format!("#{h}"),
    }
}
