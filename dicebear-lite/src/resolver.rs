//! Resolves variants, transforms, and colors against the PRNG.
//!
//! Every draw is keyed by `(seed, key)` and therefore deterministic regardless
//! of call order, so no memoization is needed.

use crate::data::{self, ComponentDef};
use crate::prng::Prng;

const BUF: usize = 32;

pub struct Resolver<'a> {
    prng: Prng<'a>,
}

pub struct ComponentTransform {
    pub rotate: f64,
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale: f64,
}

/// Follows a component's `extends` alias to its source (one hop).
fn resolve(comp: &ComponentDef) -> &ComponentDef {
    if let Some(source) = comp.extends
        && let Some(resolved) = data::component(source)
    {
        return resolved;
    }
    comp
}

impl<'a> Resolver<'a> {
    pub const fn new(seed: &'a str) -> Self {
        Self {
            prng: Prng::new(seed),
        }
    }

    /// Selects a variant for `name`, or `None` when the component is not
    /// visible. PRNG keys use `name` (the alias); the probability and variant
    /// pool come from the resolved source.
    pub fn variant(&self, name: &str) -> Option<&'static str> {
        let component = data::component(name)?;
        let resolved = resolve(component);
        let probability = resolved.probability.unwrap_or(100.0);
        if !self.prng.bool(&[name, "Probability"], probability) {
            return None;
        }
        let n = resolved.variants.len();
        let mut entries: [(&'static str, f64); BUF] = [("", 0.0); BUF];
        for (i, (variant_name, def)) in resolved.variants.iter().take(n).enumerate() {
            entries[i] = (*variant_name, def.weight);
        }
        self.prng.weighted_pick(&[name, "Variant"], &entries[..n])
    }

    /// Rotate/translate/scale for one component. Ranges come from the resolved
    /// source; PRNG keys use `name`. Absent ranges fall back without drawing.
    pub fn component_transform(&self, name: &str) -> ComponentTransform {
        let resolved = data::component(name).map(resolve);
        ComponentTransform {
            rotate: resolved.and_then(|c| c.rotate).map_or(0.0, |(min, max)| {
                self.prng.float(&[name, "Rotate"], min, max)
            }),
            translate_x: resolved
                .and_then(|c| c.translate)
                .map_or(0.0, |((min, max), _)| {
                    self.prng.float(&[name, "TranslateX"], min, max)
                }),
            translate_y: resolved
                .and_then(|c| c.translate)
                .map_or(0.0, |(_, (min, max))| {
                    self.prng.float(&[name, "TranslateY"], min, max)
                }),
            scale: resolved.and_then(|c| c.scale).map_or(1.0, |(min, max)| {
                self.prng.float(&[name, "Scale"], min, max)
            }),
        }
    }

    /// Resolves a named color to its single shuffled stop (solid fills only).
    pub fn color(&self, name: &str) -> &'static str {
        let palette = match name {
            "background" => data::BG_COLORS,
            "constellation" => data::CON_COLORS,
            _ => return "",
        };
        if palette.is_empty() {
            return "";
        }
        let index = self.prng.shuffle_zero(&[name, "Color"], palette.len());
        palette[index.min(palette.len() - 1)]
    }
}
