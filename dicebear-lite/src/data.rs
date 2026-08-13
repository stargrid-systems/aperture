// Shared SVG data model used by style definitions and the renderer.

use core::ops::Deref;

/// Color palette stops. Limited to 8 entries, enforced at construction.
#[derive(Clone, Copy)]
pub struct Palette<'a> {
    stops: &'a [&'a str],
}

impl<'a> Palette<'a> {
    pub const fn new(stops: &'a [&'a str]) -> Self {
        assert!(stops.len() <= 8, "palette exceeds 8 entries");
        Self { stops }
    }
}

impl<'a> Deref for Palette<'a> {
    type Target = [&'a str];
    fn deref(&self) -> &Self::Target {
        self.stops
    }
}

#[derive(Clone, Copy)]
pub struct ColorRef<'a> {
    pub key: &'a str,
    pub palette: Palette<'a>,
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
    Component {
        name: &'a str,
        component: &'a ComponentDef<'a>,
        attrs: &'a [(&'a str, AttrVal<'a>)],
    },
}

pub struct VariantDef<'a> {
    pub name: &'a str,
    pub weight: f64,
    pub elements: &'a [Node<'a>],
}

/// Variant definitions, verified sorted by name at construction.
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
    pub translate: Option<((f64, f64), (f64, f64))>,
    pub rotate: Option<(f64, f64)>,
    pub scale: Option<(f64, f64)>,
    pub variants: Variants<'a>,
}
