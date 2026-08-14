//! WCAG color helpers, a direct port of `DiceBear`'s `Utils/Color`.
//!
//! The sRGB linearization table is transcribed verbatim (instead of computed)
//! because `pow` is not correctly rounded: results must match the other
//! `DiceBear` ports bit for bit.

use core::fmt;

use self::linearized::LINEARIZED;

mod linearized;

/// An 8-bit-per-channel RGB color. [`Display`](core::fmt::Display) renders
/// the `#rrggbb` form `DiceBear` emits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb8 {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb8 {
    /// Builds a color from its packed 24-bit form (`0xrrggbb`).
    ///
    /// # Panics
    ///
    /// Panics in debug builds when `value` exceeds 24 bits.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "value is masked to 24 bits first"
    )]
    pub const fn from_u24(value: u32) -> Self {
        debug_assert!(value < 0x0100_0000, "rgb value exceeds 24 bits");
        let value = value & 0x00FF_FFFF;
        Self {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }
    }

    /// WCAG 2.1 relative luminance.
    #[cfg_attr(
        test,
        expect(clippy::suboptimal_flops, reason = "no FMA: matches JavaScript")
    )]
    #[must_use]
    pub fn luminance(self) -> f64 {
        0.2126 * LINEARIZED[usize::from(self.r)]
            + 0.7152 * LINEARIZED[usize::from(self.g)]
            + 0.0722 * LINEARIZED[usize::from(self.b)]
    }

    /// WCAG contrast ratio between this color and `other`.
    #[must_use]
    pub fn contrast_ratio(self, other: Self) -> f64 {
        let a = self.luminance();
        let b = other.luminance();
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

impl fmt::Display for Rgb8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}
