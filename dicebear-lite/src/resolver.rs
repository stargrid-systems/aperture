//! Resolves variants, transforms, and colors against the PRNG.
//!
//! Every draw is keyed by `(seed, key)` and therefore deterministic regardless
//! of call order, so no memoization is needed.

use crate::color::to_hex;
use crate::definition::{Range, STYLE};
use crate::prng::Prng;

pub struct Resolver {
    prng: Prng,
}

pub struct ComponentTransform {
    pub rotate: f64,
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale: f64,
}

impl Resolver {
    pub fn new(seed: &str) -> Self {
        Self {
            prng: Prng::new(seed),
        }
    }

    /// Selects a variant for `name`, or `None` when the component is not
    /// visible this render. PRNG keys use `name` (the alias, e.g. `star02`);
    /// the probability and variant pool come from the resolved source.
    pub fn variant(&self, name: &str) -> Option<String> {
        let component = STYLE.components.get(name)?;
        let resolved = component.resolve(&STYLE.components);
        let probability = resolved.probability.unwrap_or(100.0);
        if !self.prng.bool(&format!("{name}Probability"), probability) {
            return None;
        }
        let entries: Vec<(String, f64)> = resolved
            .variants
            .iter()
            .map(|(n, v)| (n.clone(), v.weight.unwrap_or(1.0)))
            .collect();
        let picked = self.prng.weighted_pick(&format!("{name}Variant"), entries);
        if picked.is_empty() {
            None
        } else {
            Some(picked)
        }
    }

    /// Rotate/translate/scale for one component. Ranges come from the resolved
    /// source; PRNG keys use `name`. Absent ranges fall back without drawing.
    pub fn component_transform(&self, name: &str) -> ComponentTransform {
        let resolved = STYLE
            .components
            .get(name)
            .map(|c| c.resolve(&STYLE.components));
        ComponentTransform {
            rotate: self.float_for(
                resolved.and_then(|c| c.rotate),
                &format!("{name}Rotate"),
                0.0,
            ),
            translate_x: self.float_for(
                resolved
                    .and_then(|c| c.translate.as_ref())
                    .and_then(|t| t.x),
                &format!("{name}TranslateX"),
                0.0,
            ),
            translate_y: self.float_for(
                resolved
                    .and_then(|c| c.translate.as_ref())
                    .and_then(|t| t.y),
                &format!("{name}TranslateY"),
                0.0,
            ),
            scale: self.float_for(resolved.and_then(|c| c.scale), &format!("{name}Scale"), 1.0),
        }
    }

    fn float_for(&self, range: Option<Range>, key: &str, fallback: f64) -> f64 {
        match range {
            Some(r) => self.prng.float(key, r.min, r.max),
            None => fallback,
        }
    }

    /// Resolves a named color to its single shuffled stop (solid fills only).
    pub fn color(&self, name: &str) -> Vec<String> {
        let Some(style_color) = STYLE.colors.get(name) else {
            return Vec::new();
        };
        let candidates: Vec<String> = style_color.values.iter().map(|c| to_hex(c)).collect();
        // Default `colorFill` is `solid` → a single stop, drawn via shuffle.
        self.prng
            .shuffle(&format!("{name}Color"), candidates)
            .into_iter()
            .take(1)
            .collect()
    }
}
