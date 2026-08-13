#![no_std]
//! Slim renderer for `DiceBear` avatars.
//!
//! Produces SVG output byte-identical to `DiceBear` 10.x for default options,
//! verified against committed reference fixtures. The PRNG, number formatting,
//! and serializer are ports of `@dicebear/core`; only the subset a style
//! exercises is implemented (no option validation, gradients-as-options, tags,
//! or text variables). Style definitions are plain Rust data behind feature
//! gates, so no JSON is parsed.
//!
//! `Avatar` implements [`core::fmt::Display`], so callers materialize the SVG
//! with `Avatar::new(seed, &STYLE).to_string()` (in `alloc`/`std`) or by
//! writing it into any [`core::fmt::Write`]. The crate itself is `no_std` and
//! alloc-free.
//!
//! `DiceBear`'s core is MIT-licensed; attribution for the ported logic belongs
//! to the `DiceBear` project (<https://www.dicebear.com>).
//!
//! # Features
//!
//! - `constellation` -- the `DiceBear` "constellation" style (CC0 1.0).

use core::fmt;

use self::data::{ComponentDef, Node};

mod data;
mod number;
mod prng;
mod renderer;
mod resolver;
mod styles;
mod xml;

#[cfg(feature = "constellation")]
pub use self::styles::constellation::CONSTELLATION;

/// A `DiceBear` style definition. Each field is `&'static` data baked into the
/// binary by the style module (e.g. [`constellation::CONSTELLATION`]).
///
/// [`constellation::CONSTELLATION`]: styles::constellation::CONSTELLATION
pub struct Style {
    pub source_name: &'static str,
    pub metadata: &'static str,
    pub canvas_w: f64,
    pub canvas_h: f64,
    pub canvas: &'static [Node],
    pub components: &'static [(&'static str, &'static ComponentDef)],
    pub palettes: &'static [(&'static str, &'static [&'static str])],
}

impl Style {
    /// Looks up a component by name.
    pub fn component(&self, name: &str) -> Option<&'static ComponentDef> {
        self.components
            .iter()
            .find_map(|(n, c)| (*n == name).then_some(*c))
    }

    /// Looks up a named color palette.
    pub fn palette(&self, name: &str) -> Option<&'static [&'static str]> {
        self.palettes
            .iter()
            .find_map(|(n, p)| (*n == name).then_some(*p))
    }

    /// Follows a component's `extends` alias to its source (one hop).
    pub fn resolve<'a>(&self, comp: &'a ComponentDef) -> &'a ComponentDef {
        comp.extends
            .and_then(|source| self.component(source))
            .unwrap_or(comp)
    }
}

/// A `DiceBear` avatar for a given `seed` and [`Style`].
///
/// Rendering is deferred to [`Display`](core::fmt::Display), so constructing
/// an `Avatar` is free. The same seed and style always produce byte-identical
/// output.
pub struct Avatar<'a> {
    seed: &'a str,
    style: &'static Style,
}

impl<'a> Avatar<'a> {
    /// Creates an avatar for `seed` using `style`.
    #[must_use]
    pub const fn new(seed: &'a str, style: &'static Style) -> Self {
        Self { seed, style }
    }
}

impl fmt::Display for Avatar<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        renderer::Renderer::new(self.seed, self.style).fmt(f)
    }
}
