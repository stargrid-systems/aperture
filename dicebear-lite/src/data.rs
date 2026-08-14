// Shared SVG data model used by style definitions and the renderer.

use core::ops::Deref;

use crate::color::Rgb8;

/// Min/max range for PRNG-driven transforms.
#[derive(Clone, Copy)]
pub struct Range(pub f64, pub f64);

/// Color palette stops. Must be non-empty and limited to `MAX_LEN` entries,
/// enforced at construction.
#[derive(Clone, Copy)]
pub struct Palette<'a> {
    stops: &'a [Rgb8],
}

impl<'a> Palette<'a> {
    pub const MAX_LEN: usize = 16;

    pub const fn new(stops: &'a [Rgb8]) -> Self {
        assert!(!stops.is_empty(), "palette must not be empty");
        assert!(stops.len() <= Self::MAX_LEN, "palette exceeds MAX_LEN");
        Self { stops }
    }
}

impl Deref for Palette<'_> {
    type Target = [Rgb8];
    fn deref(&self) -> &Self::Target {
        self.stops
    }
}

#[derive(Clone, Copy)]
pub struct ColorRef<'a> {
    pub key: &'a str,
    pub palette: Palette<'a>,
    /// Sort this color's stops by descending contrast against the first stop
    /// of the referenced color, skipping the shuffle (`DiceBear`
    /// `contrastTo`).
    pub contrast_to: Option<&'a str>,
    /// Drop stops equal to any resolved stop of the referenced colors
    /// (`DiceBear` `notEqualTo`).
    pub not_equal_to: &'a [&'a str],
}

#[derive(Clone, Copy)]
pub enum AttrVal<'a> {
    Lit(&'a str),
    Color(ColorRef<'a>),
}

pub enum Node<'a> {
    El {
        name: &'a str,
        attrs: &'a [(&'a str, AttrVal<'a>)],
        children: &'a [Self],
    },
    /// Escaped character data inside the parent element.
    Text { value: &'a str },
    Component {
        name: &'a str,
        component: &'a ComponentDef<'a>,
        attrs: &'a [(&'a str, AttrVal<'a>)],
    },
}

pub struct VariantDef<'a> {
    pub name: &'a str,
    pub weight: f64,
    /// Variant tags, e.g. `&["animation"]` on animation-speed variants. Drives
    /// [`Animation::Random`](crate::Animation::Random) selection.
    pub tags: &'a [&'a str],
    pub elements: &'a [Node<'a>],
}

/// Variant definitions, verified sorted by name at construction. The sort
/// order is load-bearing: the resolver's weighted pick walks variants in
/// definition order, so the order must match `DiceBear`'s name-sorted
/// iteration.
#[derive(Clone, Copy)]
pub struct Variants<'a> {
    variants: &'a [VariantDef<'a>],
}

impl<'a> Variants<'a> {
    pub const fn new(variants: &'a [VariantDef<'a>]) -> Self {
        let mut i = 1;
        while i < variants.len() {
            let prev = variants[i - 1].name.as_bytes();
            let curr = variants[i].name.as_bytes();
            let mut j = 0;
            loop {
                if j >= prev.len() {
                    break;
                }
                assert!(j < curr.len(), "variants must be sorted by name");
                assert!(prev[j] <= curr[j], "variants must be sorted by name");
                if prev[j] < curr[j] {
                    break;
                }
                j += 1;
            }
            i += 1;
        }
        Self { variants }
    }
}

impl<'a> Deref for Variants<'a> {
    type Target = [VariantDef<'a>];
    fn deref(&self) -> &Self::Target {
        self.variants
    }
}

pub struct ComponentDef<'a> {
    pub name: &'a str,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub probability: Option<f64>,
    pub translate: Option<(Range, Range)>,
    pub rotate: Option<Range>,
    pub variants: Variants<'a>,
}

/// Canvas node slice. Limited to `MAX_LEN` nodes, enforced at construction.
pub struct Canvas<'a> {
    nodes: &'a [Node<'a>],
}

impl<'a> Canvas<'a> {
    pub const MAX_LEN: usize = 32;

    pub const fn new(nodes: &'a [Node<'a>]) -> Self {
        assert!(nodes.len() <= Self::MAX_LEN, "canvas exceeds MAX_LEN");
        Self { nodes }
    }
}

impl<'a> Deref for Canvas<'a> {
    type Target = [Node<'a>];
    fn deref(&self) -> &Self::Target {
        self.nodes
    }
}
