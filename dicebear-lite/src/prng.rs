//! `DiceBear`'s key-based PRNG: FNV-1a over UTF-16 code units seeding
//! Mulberry32.
//!
//! Every draw is derived independently from `(seed, key)`, so the call order
//! is irrelevant. A key is several string fragments concatenated, hashed
//! incrementally so no allocation is needed.

use core::iter;

use crate::Animation;
use crate::color::Rgb8;
use crate::data::{ColorRef, ComponentDef, Palette, VariantDef};
use crate::number::{equals, floor_index, math_round};

const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// Resolved `not_equal_to` stops tracked per color, alloc-free.
const MAX_COLOR_REFS: usize = 8;

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
        math_round((min + self.value(key) * (max - min)) * 10000.0) / 10000.0
    }

    /// Returns the element that lands at position 0 after a Fisher-Yates
    /// shuffle of `[0, 1, ..., n-1]`.
    ///
    /// # Panics
    ///
    /// Panics if `n == 0` or `n > Palette::MAX_LEN`.
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
    /// `animation` mirrors `DiceBear`'s opt-in animation options.
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

    /// Weighted pick matching `Prng.weightedPick`: a zero total weight makes
    /// the pick uniform by index. At least one variant must match.
    fn weighted<'b>(
        &self,
        key: &[&str],
        variants: &'b [VariantDef<'b>],
        pred: impl Fn(&VariantDef) -> bool,
    ) -> &'b VariantDef<'b> {
        let matching = || variants.iter().filter(|v| pred(v));
        let total: f64 = matching().map(|v| v.weight).sum();
        let value = self.value(key);

        if equals(total, 0.0) {
            let index = floor_index(value, matching().count());
            return matching().nth(index).expect("non-empty filtered set");
        }

        let mut cumulative = 0.0;
        for variant in matching() {
            cumulative += variant.weight;
            if value * total < cumulative {
                return variant;
            }
        }
        matching().last().expect("non-empty filtered set")
    }

    /// Resolves `color` to its output stop, porting `DiceBear`'s
    /// `Resolver.resolveColor` for default options: `not_equal_to` stops are
    /// dropped (falling back to the full palette when that would empty it), a
    /// `contrast_to` reference sorts the stops by descending WCAG contrast
    /// (stable, no shuffle), otherwise the surviving stops are shuffled.
    pub fn resolve(&self, color: &ColorRef<'_>) -> Rgb8 {
        let palette: &[Rgb8] = &color.palette;

        let mut excluded: [Rgb8; MAX_COLOR_REFS] = [Rgb8::from_u24(0); MAX_COLOR_REFS];
        let mut excluded_count = 0;
        for reference in color.not_equal_to {
            debug_assert!(excluded_count < MAX_COLOR_REFS, "too many references");
            excluded[excluded_count] = self.resolve(reference);
            excluded_count += 1;
        }
        let excluded = &excluded[..excluded_count];
        let is_kept = |stop: &Rgb8| !excluded.contains(stop);
        let kept: usize = palette.iter().filter(|stop| is_kept(stop)).count();

        if let Some(reference) = color.contrast_to {
            let ref_color = self.resolve(reference);
            let mut ranked: [usize; Palette::MAX_LEN] = [0; Palette::MAX_LEN];
            for (i, slot) in ranked.iter_mut().enumerate().take(palette.len()) {
                *slot = i;
            }
            let ranked = &mut ranked[..palette.len()];
            for i in 1..ranked.len() {
                let mut j = i;
                while j > 0
                    && palette[ranked[j]].contrast_ratio(ref_color)
                        > palette[ranked[j - 1]].contrast_ratio(ref_color)
                {
                    ranked.swap(j, j - 1);
                    j -= 1;
                }
            }
            let first_kept = ranked.iter().copied().find(|&i| is_kept(&palette[i]));
            return palette[first_kept.unwrap_or(ranked[0])];
        }

        // The shuffle is drawn over the surviving stops, or the whole palette
        // when nothing survives.
        let mut pool: [Rgb8; Palette::MAX_LEN] = [Rgb8::from_u24(0); Palette::MAX_LEN];
        let mut len = 0;
        for stop in palette {
            if kept == 0 || is_kept(stop) {
                pool[len] = *stop;
                len += 1;
            }
        }
        debug_assert!(len > 0, "palette is never empty");
        pool[self.shuffle_zero(&[color.key, "Color"], len)]
    }
}
