//! Slim renderer for `DiceBear` avatars.
//!
//! Produces SVG output byte-identical to `DiceBear` 10.x for default options,
//! verified against committed reference fixtures. The PRNG, number formatting,
//! and serializer are ports of `@dicebear/core`. Only the subset a style
//! exercises is implemented (no option validation, gradients-as-options, tags,
//! or text variables). Style definitions are plain Rust data behind feature
//! gates, so no JSON is parsed.
//!
//! `Avatar` implements [`core::fmt::Display`], so callers materialize the SVG
//! with `Avatar::new(&STYLE, seed).to_string()` (in `alloc`/`std`) or by
//! writing it into any [`core::fmt::Write`]. The crate itself is `no_std` and
//! alloc-free.
//!
//! `DiceBear`'s core is MIT-licensed. Attribution for the ported logic belongs
//! to the `DiceBear` project (<https://www.dicebear.com>).
//!
//! # Features
//!
//! - `constellation`: enables the `DiceBear` "constellation" style (CC0 1.0).

#![cfg_attr(not(test), no_std)]

use core::fmt;

use self::data::{Canvas, ColorRef};
#[cfg(feature = "constellation")]
pub use self::styles::constellation::CONSTELLATION;

mod data;
mod number;
mod prng;
mod renderer;
mod resolver;
mod styles;
mod svg;

#[doc(hidden)]
pub mod internal {
    pub use crate::number::Num;
    pub use crate::prng::{Prng, hash_u32};
    pub use crate::svg::Escaped;
}

/// A `DiceBear` style definition. Each field is `&'a` data baked into the
/// binary by the style module (e.g. [`constellation::CONSTELLATION`]).
///
/// [`constellation::CONSTELLATION`]: styles::constellation::CONSTELLATION
pub struct Style<'a> {
    pub(crate) source_name: &'a str,
    pub(crate) metadata: &'a str,
    pub(crate) canvas_w: f64,
    pub(crate) canvas_h: f64,
    pub(crate) canvas: Canvas<'a>,
    pub(crate) background: ColorRef<'a>,
}

/// A `DiceBear` avatar for a given `seed` and [`Style`].
///
/// Rendering is deferred to [`Display`](core::fmt::Display), so constructing an
/// `Avatar` is free. The same seed and style always produce byte-identical
/// output.
pub struct Avatar<'a> {
    style: &'a Style<'a>,
    seed: &'a str,
}

impl<'a> Avatar<'a> {
    /// Creates an avatar for `seed` using `style`.
    #[must_use]
    pub const fn new(style: &'a Style<'a>, seed: &'a str) -> Self {
        Self { style, seed }
    }
}

impl fmt::Display for Avatar<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        renderer::Renderer::new(self.style, self.seed).fmt(f)
    }
}
