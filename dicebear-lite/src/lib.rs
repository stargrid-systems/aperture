//! Slim renderer for `DiceBear` "constellation" avatars.
//!
//! Produces SVG output byte-identical to `DiceBear` 10.x for default options,
//! verified against committed reference fixtures. The PRNG, number formatting,
//! and serializer are ports of `@dicebear/core`; only the subset constellation
//! exercises is implemented (no option validation, gradients, tags, or text
//! variables). The style definition is `DiceBear`'s own `constellation.json`,
//! embedded at compile time.
//!
//! `DiceBear`'s core is MIT-licensed; attribution for the ported logic belongs
//! to the `DiceBear` project (<https://www.dicebear.com>). The constellation
//! style is CC0 1.0.

mod color;
mod definition;
mod number;
mod prng;
mod renderer;
mod resolver;
mod xml;

/// Renders a deterministic constellation avatar for `seed` as an SVG document.
///
/// The same `seed` always produces byte-identical output, matching `DiceBear`.
#[must_use]
pub fn constellation(seed: &str) -> String {
    let resolver = resolver::Resolver::new(seed);
    renderer::Renderer::new(&resolver, seed).render()
}
