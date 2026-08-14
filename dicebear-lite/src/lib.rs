//! Slim renderer for `DiceBear` avatars.
//!
//! Produces SVG output byte-identical to `DiceBear` 10.x for default options,
//! verified against the upstream crates at test time. The PRNG, number
//! formatting, and serializer are ports of `@dicebear/core`; only the subset a
//! style exercises is implemented. Style definitions are plain Rust data
//! behind feature gates, so no JSON is parsed.
//!
//! `Avatar` implements [`core::fmt::Display`], so callers materialize the SVG
//! with `Avatar::new(&STYLE, seed).to_string()` or by writing it into any
//! [`core::fmt::Write`]. The crate is `no_std` and alloc-free. `DiceBear`'s
//! core is MIT-licensed; attribution for the ported logic belongs to the
//! `DiceBear` project (<https://www.dicebear.com>).
//!
//! # Features
//!
//! - `constellation`: enables the `DiceBear` "constellation" style (CC0 1.0).
//! - `planets`: enables the `DiceBear` "planets" style (CC0 1.0).
//! - `thumbs`: enables the `DiceBear` "thumbs" style (CC0 1.0).
//!
//! # Animation
//!
//! `Animation` mirrors `DiceBear`'s opt-in `tags: ["animation"]` and
//! `animationVariant` options. [`Animation::Off`] (the default) renders the
//! static avatar.

#![cfg_attr(not(test), no_std)]

use core::fmt;

use self::data::{Canvas, ColorRef};

mod color;
mod data;
mod number;
mod prng;
mod renderer;

#[cfg(feature = "constellation")]
pub use self::styles::constellation::CONSTELLATION;
#[cfg(feature = "planets")]
pub use self::styles::planets::PLANETS;
#[cfg(feature = "thumbs")]
pub use self::styles::thumbs::THUMBS;

mod styles {
    #[cfg(feature = "constellation")]
    pub mod constellation;
    #[cfg(feature = "planets")]
    pub mod planets;
    #[cfg(feature = "thumbs")]
    pub mod thumbs;
}

/// A `DiceBear` style definition. Each field is `&'a` data baked into the
/// binary by a style module (e.g. `CONSTELLATION`).
pub struct Style<'a> {
    pub(crate) source_name: &'a str,
    pub(crate) metadata: &'a str,
    pub(crate) canvas_w: f64,
    pub(crate) canvas_h: f64,
    pub(crate) canvas: Canvas<'a>,
    pub(crate) background: ColorRef<'a>,
}

/// Animation speed for [`Animation::Fixed`], named after the `animation`
/// component variants of the `DiceBear` styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Fastest,
    Fast,
    Medium,
    Slow,
    Slowest,
}

impl Speed {
    /// The variant name as it appears in a style definition.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fastest => "fastest",
            Self::Fast => "fast",
            Self::Medium => "medium",
            Self::Slow => "slow",
            Self::Slowest => "slowest",
        }
    }
}

/// Opt-in avatar animation, mirroring `DiceBear`'s `tags` and
/// `animationVariant` options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animation {
    /// Static avatars (the `DiceBear` default).
    Off,
    /// `tags: ["animation"]`: animated, speed drawn per seed.
    Random,
    /// `animationVariant: <speed>`: animated at a fixed speed.
    Fixed(Speed),
}

/// A `DiceBear` avatar for a given `seed`, [`Style`], and [`Animation`].
/// Rendering is deferred to [`Display`](core::fmt::Display), so construction
/// is free.
pub struct Avatar<'a> {
    style: &'a Style<'a>,
    seed: &'a str,
    animation: Animation,
}

impl<'a> Avatar<'a> {
    /// Creates an avatar for `seed` using `style`, animation off.
    #[must_use]
    pub const fn new(style: &'a Style<'a>, seed: &'a str) -> Self {
        Self {
            style,
            seed,
            animation: Animation::Off,
        }
    }

    /// Sets the animation, consuming and returning the avatar.
    #[must_use]
    pub const fn animation(mut self, animation: Animation) -> Self {
        self.animation = animation;
        self
    }
}

impl fmt::Display for Avatar<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        renderer::render(f, self.style, self.seed, self.animation)
    }
}
