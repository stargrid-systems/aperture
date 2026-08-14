//! Slim renderer for `DiceBear` avatars.
//!
//! Produces SVG output byte-identical to `DiceBear` 10.x for default options,
//! verified against the upstream crates at test time. The PRNG, number
//! formatting, and serializer are ports of `@dicebear/core`. Only the subset a
//! style exercises is implemented (no option validation, gradients-as-options,
//! tags beyond `animation`, or text variables). Style definitions are plain
//! Rust data behind feature gates, so no JSON is parsed.
//!
//! `Avatar` implements [`core::fmt::Display`], so callers materialize the SVG
//! with `Avatar::new(&STYLE, seed).to_string()` (in `alloc`/`std`) or by
//! writing it into any [`core::fmt::Write`]. Construction is free: rendering
//! is deferred to `Display`. The crate itself is `no_std` and alloc-free.
//!
//! `DiceBear`'s core is MIT-licensed. Attribution for the ported logic belongs
//! to the `DiceBear` project (<https://www.dicebear.com>).
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
//! `animationVariant` options: styles ship an `animation` component whose
//! speed variants carry zero weight, so [`Animation::Off`] (the default)
//! renders the static avatar.

#![cfg_attr(not(test), no_std)]

use core::fmt;

use self::data::{Canvas, ColorRef};

mod color;
mod data;
mod number;
mod prng;
mod renderer;

mod styles {
    #[cfg(feature = "constellation")]
    pub mod constellation;
    #[cfg(feature = "planets")]
    pub mod planets;
    #[cfg(feature = "thumbs")]
    pub mod thumbs;
}
#[cfg(feature = "constellation")]
pub use self::styles::constellation::CONSTELLATION;
#[cfg(feature = "planets")]
pub use self::styles::planets::PLANETS;
#[cfg(feature = "thumbs")]
pub use self::styles::thumbs::THUMBS;

/// A `DiceBear` style definition. Each field is `&'a` data baked into the
/// binary by a style module (e.g. `CONSTELLATION`).
pub struct Style<'a> {
    pub(crate) source_name: &'a str,
    pub(crate) metadata: &'a str,
    pub(crate) canvas_w: f64,
    pub(crate) canvas_h: f64,
    pub(crate) canvas: Canvas<'a>,
    /// Every named color, including `background`. Colors may reference each
    /// other through `contrast_to`/`not_equal_to`.
    pub(crate) colors: &'a [ColorRef<'a>],
}

impl Style<'_> {
    /// The named color definition for `name`, if the style defines it.
    pub(crate) fn color(&self, name: &str) -> Option<&ColorRef<'_>> {
        self.colors.iter().find(|color| color.key == name)
    }
}

/// Animation speed for [`Animation::Fixed`].
///
/// The names match the `animation` component variants of the `DiceBear`
/// styles.
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
///
/// [`Animation::Random`] turns on every component variant tagged `animation`
/// (for the shipped styles: the animation component, at a per-seed speed).
/// [`Animation::Fixed`] pins the speed instead; styles without an `animation`
/// component render statically either way.
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
///
/// Rendering is deferred to [`Display`](core::fmt::Display), so constructing an
/// `Avatar` is free. The same seed, style, and animation always produce
/// byte-identical output.
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
