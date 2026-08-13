#![no_std]
//! Slim renderer for `DiceBear` "constellation" avatars.
//!
//! Produces SVG output byte-identical to `DiceBear` 10.x for default options,
//! verified against committed reference fixtures. The PRNG, number formatting,
//! and serializer are ports of `@dicebear/core`; only the subset constellation
//! exercises is implemented (no option validation, gradients, tags, or text
//! variables). The style definition is plain Rust data — no JSON is parsed.
//!
//! `Avatar` implements [`core::fmt::Display`], so callers materialize the SVG
//! with `Avatar::new(seed).to_string()` (in `alloc`/`std`) or by writing it
//! into any [`core::fmt::Write`]. The crate itself is `no_std` and alloc-free.
//!
//! `DiceBear`'s core is MIT-licensed; attribution for the ported logic belongs
//! to the `DiceBear` project (<https://www.dicebear.com>). The constellation
//! style is CC0 1.0.

mod data;
mod number;
mod prng;
mod renderer;
mod resolver;
mod xml;

use core::fmt;

/// A `DiceBear` "constellation" avatar for a given seed.
///
/// Rendering is deferred to [`Display`](core::fmt::Display), so constructing an
/// `Avatar` is free. The same seed always produces byte-identical output.
pub struct Avatar<'a> {
    seed: &'a str,
}

impl<'a> Avatar<'a> {
    /// Creates an avatar for `seed`.
    #[must_use]
    pub const fn new(seed: &'a str) -> Self {
        Self { seed }
    }
}

impl fmt::Display for Avatar<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        renderer::render(self.seed, f)
    }
}
