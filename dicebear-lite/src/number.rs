//! Number formatting that matches `DiceBear`'s cross-language output exactly.
//!
//! `core` does not provide float `trunc`/`floor`/`mul_add` (they need libm), so
//! the few we need are implemented here. All values we handle are bounded to
//! the SVG coordinate range, so `as i64` truncation is exact.

use core::{fmt, str};

/// Replicates JavaScript `Math.round`: halves round toward `+Infinity`.
pub fn math_round(x: f64) -> f64 {
    let tr = trunc(x);
    let fr = x - tr;
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

/// Truncates toward zero. Inputs are bounded to `i64` range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "bounded SVG-range values"
)]
const fn trunc(x: f64) -> f64 {
    (x as i64) as f64
}

/// Floors toward negative infinity.
fn floor(x: f64) -> f64 {
    let t = trunc(x);
    if x >= 0.0 || equals(x, t) { t } else { t - 1.0 }
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
    (unit * len as f64) as i64 as usize
}

/// A number formatted like `DiceBear`'s `Utils/Number.format`: rounded to at
/// most 5 decimal places, trailing zeros trimmed, no exponential notation.
pub struct Num(pub f64);

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "abs of bounded SVG-range values to integer digits"
)]
impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        if value.is_nan() {
            return f.write_str("NaN");
        }
        if value.is_infinite() {
            return f.write_str(if value < 0.0 { "-Infinity" } else { "Infinity" });
        }

        let mut scaled = math_round(value * 100_000.0);
        if scaled < 0.0 {
            f.write_str("-")?;
            scaled = -scaled;
        }

        let integer = floor(scaled / 100_000.0) as u64;
        let fracval = (scaled % 100_000.0) as u64;
        write!(f, "{integer}")?;
        if fracval == 0 {
            return Ok(());
        }

        let mut digits = [b'0'; 5];
        let mut v = fracval;
        let mut i = 5;
        while i > 0 {
            i -= 1;
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        let mut end = 5;
        while end > 0 && digits[end - 1] == b'0' {
            end -= 1;
        }
        write!(
            f,
            ".{}",
            str::from_utf8(&digits[..end]).expect("ascii digits")
        )
    }
}
