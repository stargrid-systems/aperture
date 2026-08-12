//! Number formatting that matches `DiceBear`'s cross-language output exactly.

/// Replicates JavaScript `Math.round`: halves round toward `+Infinity`.
pub fn math_round(x: f64) -> f64 {
    let tr = x.trunc();
    let fr = x.fract();
    if x >= 0.0 {
        if fr >= 0.5 { tr + 1.0 } else { tr }
    } else if fr >= -0.5 {
        tr
    } else {
        tr - 1.0
    }
}

/// Exact float equality, matching JavaScript `===`. Values drawn from the PRNG
/// are rounded to multiples of `1e-4`, so exact comparison is correct.
#[expect(
    clippy::float_cmp,
    reason = "intentional exact equality matching JS ==="
)]
pub fn equals(a: f64, b: f64) -> bool {
    a == b
}

/// Scales `unit` (a `[0, 1)` float) to an index in `0..len`, matching
/// `Math.floor(unit * len)`.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "JS index computation over a small length"
)]
pub fn floor_index(unit: f64, len: usize) -> usize {
    (unit * len as f64).floor() as usize
}

/// Converts a non-negative rounded `f64` to `u64` for digit extraction.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "abs of a rounded value in SVG coordinate range"
)]
const fn to_u64(x: f64) -> u64 {
    x as u64
}

/// Formats `value` rounded to at most 5 decimal places, trailing zeros
/// trimmed, no exponential notation. Mirrors `Utils/Number.format`.
pub fn format(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 {
            "-Infinity".to_owned()
        } else {
            "Infinity".to_owned()
        };
    }

    let mut scaled = math_round(value * 100_000.0);
    let sign = if scaled < 0.0 { "-" } else { "" };
    scaled = scaled.abs();

    let integer_part = to_u64((scaled / 100_000.0).floor());
    let mut frac = format!("{:05}", to_u64(scaled % 100_000.0));
    while frac.ends_with('0') {
        frac.pop();
    }

    if frac.is_empty() {
        format!("{sign}{integer_part}")
    } else {
        format!("{sign}{integer_part}.{frac}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_integers_and_decimals() {
        assert_eq!(format(0.0), "0");
        assert_eq!(format(100.0), "100");
        assert_eq!(format(1.2948), "1.2948");
        assert_eq!(format(-1.2948), "-1.2948");
        assert_eq!(format(127.6635), "127.6635");
    }

    #[test]
    fn rounds_half_toward_positive_infinity() {
        assert_eq!(math_round(2.5).to_bits(), 3.0f64.to_bits());
        assert_eq!(math_round(-2.5).to_bits(), (-2.0f64).to_bits());
        assert_eq!(math_round(2.4).to_bits(), 2.0f64.to_bits());
        assert_eq!(math_round(-2.6).to_bits(), (-3.0f64).to_bits());
    }
}
