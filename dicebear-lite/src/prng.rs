//! `DiceBear`'s key-based PRNG: FNV-1a over UTF-16 code units seeding
//! Mulberry32.
//!
//! Every draw is derived independently from `(seed, key)`, so the call order is
//! irrelevant. A "key" is several string fragments concatenated. Hashing walks
//! them incrementally so no allocation is needed. This is a direct port of
//! `@dicebear/prng`.

use core::iter;

use crate::Animation;
use crate::color::{contrast_ratio, same_rgb};
use crate::data::{ColorRef, ComponentDef, Palette, VariantDef};
use crate::number::{equals, floor_index, math_round};

const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// Resolved `not_equal_to` stops tracked per color, alloc-free.
const MAX_REFS: usize = 8;

#[inline]
fn step(hash: u32, unit: u16) -> u32 {
    (hash ^ u32::from(unit)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a over the UTF-16 code units of `seed`, a `:` separator, then the
/// concatenated `key` fragments.
fn hash_seed_key(seed: &str, key: &[&str]) -> u32 {
    seed.encode_utf16()
        .chain(iter::once(0x3A_u16))
        .chain(key.iter().flat_map(|s| s.encode_utf16()))
        .fold(FNV_OFFSET, step)
}

/// FNV-1a hash of `prefix + ":" + s` over UTF-16 code units. Used for the
/// per-seed `<defs>` id suffix.
pub fn hash_u32(prefix: &str, s: &str) -> u32 {
    prefix
        .encode_utf16()
        .chain(iter::once(0x3A_u16))
        .chain(s.encode_utf16())
        .fold(FNV_OFFSET, step)
}

/// Stateful Mulberry32 generator.
struct Mulberry32 {
    state: u32,
}

impl Mulberry32 {
    const fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    const fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6D2B_79F5);
        let z = self.state;
        let mut t = (z ^ (z >> 15)).wrapping_mul(z | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 0x3D));
        t ^ (t >> 14)
    }

    fn next_float(&mut self) -> f64 {
        f64::from(self.next()) / 4_294_967_296.0
    }
}

/// Key-based pseudorandom value generator. Keys are passed as multiple
/// fragments (e.g. `&[name, "Variant"]`) and concatenated for hashing.
pub struct Prng<'a> {
    seed: &'a str,
}

impl<'a> Prng<'a> {
    pub const fn new(seed: &'a str) -> Self {
        Self { seed }
    }

    /// Raw `[0, 1)` value for `key`.
    pub fn value(&self, key: &[&str]) -> f64 {
        Mulberry32::new(hash_seed_key(self.seed, key)).next_float()
    }

    pub fn bool(&self, key: &[&str], likelihood: f64) -> bool {
        self.value(key) * 100.0 < likelihood
    }

    #[cfg_attr(
        test,
        expect(clippy::suboptimal_flops, reason = "no FMA: matches JavaScript")
    )]
    pub fn float(&self, key: &[&str], min: f64, max: f64) -> f64 {
        // No fused multiply-add: the product and sum are computed separately,
        // matching JavaScript semantics.
        math_round((min + self.value(key) * (max - min)) * 10000.0) / 10000.0
    }

    /// Returns the element that lands at position 0 after a Fisher-Yates
    /// shuffle of `[0, 1, ..., n-1]`. Instead of tracing swap chains, the full
    /// permutation is materialized in a fixed-size array.
    ///
    /// # Panics
    ///
    /// Panics if `n > Palette::MAX_LEN`.
    pub fn shuffle_zero(&self, key: &[&str], n: usize) -> usize {
        assert!(n <= Palette::MAX_LEN);
        let mut rng = Mulberry32::new(hash_seed_key(self.seed, key));
        let mut perm = [0usize; Palette::MAX_LEN];
        for (i, slot) in perm.iter_mut().enumerate().take(n) {
            *slot = i;
        }
        for i in (1..n).rev() {
            let j = floor_index(rng.next_float(), i + 1);
            perm.swap(i, j);
        }
        perm[0]
    }

    /// Selects a variant for `component`, or `None` when it is not visible.
    ///
    /// `animation` mirrors `DiceBear`'s opt-in animation options: `Random`
    /// restricts components with `animation`-tagged variants to those (their
    /// zero weights make the pick uniform, at a per-seed speed), and `Fixed`
    /// pins the variant of the `animation` component by name.
    pub fn variant<'b>(
        &self,
        name: &str,
        component: &'b ComponentDef<'b>,
        animation: Animation,
    ) -> Option<&'b VariantDef<'b>> {
        let variants = &component.variants;
        if variants.is_empty() {
            return None;
        }
        if !self.bool(
            &[name, "Probability"],
            component.probability.unwrap_or(100.0),
        ) {
            return None;
        }
        match animation {
            Animation::Off => Some(self.weighted(&[name, "Variant"], variants, |_| true)),
            Animation::Random if variants.iter().any(|v| v.tags.contains(&"animation")) => {
                Some(self.weighted(&[name, "Variant"], variants, |v| {
                    v.tags.contains(&"animation")
                }))
            }
            Animation::Fixed(speed) if component.name == "animation" => {
                variants.iter().find(|v| v.name == speed.as_str())
            }
            Animation::Random | Animation::Fixed(_) => {
                Some(self.weighted(&[name, "Variant"], variants, |_| true))
            }
        }
    }

    /// Weighted pick over the variants matching `pred`, matching
    /// `Prng.weightedPick`: with a zero total weight the pick is uniform by
    /// index. At least one variant must match.
    fn weighted<'b>(
        &self,
        key: &[&str],
        variants: &'b [VariantDef<'b>],
        pred: impl Fn(&VariantDef) -> bool,
    ) -> &'b VariantDef<'b> {
        let total: f64 = variants.iter().filter(|v| pred(v)).map(|v| v.weight).sum();
        let value = self.value(key);
        if equals(total, 0.0) {
            let len = variants.iter().filter(|v| pred(v)).count();
            let index = floor_index(value, len);
            variants
                .iter()
                .filter(|v| pred(v))
                .nth(index)
                .expect("non-empty filtered set")
        } else {
            let threshold = value * total;
            let mut cumulative = 0.0;
            for v in variants.iter().filter(|v| pred(v)) {
                cumulative += v.weight;
                if threshold < cumulative {
                    return v;
                }
            }
            variants
                .iter()
                .filter(|v| pred(v))
                .last()
                .expect("non-empty filtered set")
        }
    }

    /// Resolves `color` to its single output stop, mirroring `DiceBear`'s
    /// `Resolver.resolveColor` for default options:
    ///
    /// - `not_equal_to` stops are dropped (falling back to the full palette
    ///   when that would empty it),
    /// - a `contrast_to` reference sorts the stops by descending WCAG contrast
    ///   against the reference's first stop (stable, and no shuffle), and
    /// - otherwise the surviving stops are shuffled.
    ///
    /// # Panics
    ///
    /// Panics on circular `contrast_to`/`not_equal_to` references.
    pub fn resolve<'b>(&self, colors: &'b [ColorRef<'b>], color: &ColorRef<'b>) -> Option<&'b str> {
        self.resolve_depth(colors, color, 0)
    }

    fn resolve_depth<'b>(
        &self,
        colors: &'b [ColorRef<'b>],
        color: &ColorRef<'b>,
        depth: usize,
    ) -> Option<&'b str> {
        assert!(depth < 8, "circular color reference at {}", color.key);
        let palette: &[&'b str] = &color.palette;

        // Resolved stops of the not_equal_to references (one per reference).
        let mut excluded: [&'b str; MAX_REFS] = [""; MAX_REFS];
        let mut excluded_count = 0;
        for ref_name in color.not_equal_to {
            if let Some(stop) = Self::lookup(colors, ref_name)
                .and_then(|referenced| self.resolve_depth(colors, referenced, depth + 1))
            {
                debug_assert!(excluded_count < MAX_REFS, "too many color references");
                excluded[excluded_count] = stop;
                excluded_count += 1;
            }
        }
        let excluded = &excluded[..excluded_count];
        let is_excluded = |stop: &str| excluded.iter().any(|e| same_rgb(e, stop));
        let kept: usize = palette.iter().filter(|stop| !is_excluded(stop)).count();

        if let Some(ref_name) = color.contrast_to {
            // Identity order, then (when the reference resolves) a stable sort
            // by descending contrast ratio.
            let mut ranked: [usize; Palette::MAX_LEN] = [0; Palette::MAX_LEN];
            for (i, slot) in ranked.iter_mut().enumerate().take(palette.len()) {
                *slot = i;
            }
            let ranked = &mut ranked[..palette.len()];
            if let Some(reference) = Self::lookup(colors, ref_name)
                .and_then(|referenced| self.resolve_depth(colors, referenced, depth + 1))
            {
                for i in 1..ranked.len() {
                    let mut j = i;
                    while j > 0
                        && contrast_ratio(palette[ranked[j]], reference)
                            > contrast_ratio(palette[ranked[j - 1]], reference)
                    {
                        ranked.swap(j, j - 1);
                        j -= 1;
                    }
                }
            }
            for &i in ranked.iter() {
                if !is_excluded(palette[i]) {
                    return Some(palette[i]);
                }
            }
            return Some(palette[ranked[0]]);
        }

        let len = if kept == 0 { palette.len() } else { kept };
        let index = self.shuffle_zero(&[color.key, "Color"], len);
        if kept == 0 {
            return palette.get(index).copied();
        }
        palette
            .iter()
            .filter(|stop| !is_excluded(stop))
            .nth(index)
            .copied()
    }

    pub fn lookup<'b>(colors: &'b [ColorRef<'b>], name: &str) -> Option<&'b ColorRef<'b>> {
        colors.iter().find(|c| c.key == name)
    }
}
