//! Resolves variants, transforms, and colors against the PRNG.
//!
//! Every draw is keyed by `(seed, key)` and therefore deterministic regardless
//! of call order, so no memoization is needed.

use crate::data::{ColorRef, ComponentDef, Range, VariantDef};
use crate::number::{equals, floor_index};
use crate::prng::Prng;

pub struct Resolver<'a> {
    prng: Prng<'a>,
}

pub struct ComponentTransform {
    pub rotate: f64,
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale: f64,
}

impl<'a> Resolver<'a> {
    pub const fn new(seed: &'a str) -> Self {
        Self {
            prng: Prng::new(seed),
        }
    }

    /// Selects a variant for `component`, or `None` when it is not visible.
    /// PRNG keys use `name` (the canvas alias).
    pub fn variant(
        &self,
        name: &str,
        component: &'a ComponentDef<'a>,
    ) -> Option<&'a VariantDef<'a>> {
        let variants = &component.variants;
        if variants.is_empty() {
            return None;
        }
        if !self.prng.bool(
            &[name, "Probability"],
            component.probability.unwrap_or(100.0),
        ) {
            return None;
        }
        let total: f64 = variants.iter().map(|v| v.weight).sum();
        let value = self.prng.value(&[name, "Variant"]);
        if equals(total, 0.0) {
            return variants.get(floor_index(value, variants.len()));
        }
        let threshold = value * total;
        let mut cumulative = 0.0;
        for v in variants.iter() {
            cumulative += v.weight;
            if threshold < cumulative {
                return Some(v);
            }
        }
        variants.last()
    }

    /// Rotate/translate/scale for one component. PRNG keys use `name`.
    pub fn component_transform(
        &self,
        name: &str,
        component: &ComponentDef<'_>,
    ) -> ComponentTransform {
        ComponentTransform {
            rotate: component.rotate.map_or(0.0, |Range(min, max)| {
                self.prng.float(&[name, "Rotate"], min, max)
            }),
            translate_x: component.translate.map_or(0.0, |(Range(min, max), _)| {
                self.prng.float(&[name, "TranslateX"], min, max)
            }),
            translate_y: component.translate.map_or(0.0, |(_, Range(min, max))| {
                self.prng.float(&[name, "TranslateY"], min, max)
            }),
            scale: component.scale.map_or(1.0, |Range(min, max)| {
                self.prng.float(&[name, "Scale"], min, max)
            }),
        }
    }

    /// Resolves `color` to its single shuffled stop.
    pub fn color<'b>(&self, color: &ColorRef<'b>) -> Option<&'b str> {
        if color.palette.is_empty() {
            return None;
        }
        let idx = self
            .prng
            .shuffle_zero(&[color.key, "Color"], color.palette.len());
        color.palette.get(idx).copied()
    }
}
