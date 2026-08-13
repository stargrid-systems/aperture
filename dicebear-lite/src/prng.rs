//! `DiceBear`'s key-based PRNG: FNV-1a over UTF-16 code units seeding
//! Mulberry32.
//!
//! Every draw is derived independently from `(seed, key)`, so the call order is
//! irrelevant. A "key" is several string fragments concatenated. Hashing walks
//! them incrementally so no allocation is needed. This is a direct port of
//! `@dicebear/prng`.

use core::iter;

use crate::number::{floor_index, math_round};

const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

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
    /// shuffle of `[0, 1, ..., n-1]`. Uses the trace-label algorithm: instead
    /// of materializing the full permutation, it traces which original element
    /// ends up at position 0 by following the chain of swaps that affect it.
    ///
    /// # Panics
    ///
    /// Panics if `n > 8` (3 bits per j value in a `u32`).
    pub fn shuffle_zero(&self, key: &[&str], n: usize) -> usize {
        assert!(n <= 8, "shuffle_zero requires n <= 8");
        let mut rng = Mulberry32::new(hash_seed_key(self.seed, key));
        let mut val_at_0: usize = 0;
        let mut j_packed: u32 = 0;

        for i in (1..n).rev() {
            let j = floor_index(rng.next_float(), i + 1);
            #[expect(clippy::cast_possible_truncation, reason = "j < n <= 8, fits in u32")]
            let j_u32 = j as u32;
            j_packed |= j_u32 << (3 * (i - 1));
            if j == 0 {
                val_at_0 = trace_label(i, n, j_packed);
            }
        }

        val_at_0
    }
}

/// Traces which original element is at `start` by following the chain of
/// Fisher-Yates swaps. At each position, finds the smallest step `k` (where
/// `k > pos`) whose random index `j_k` targeted `pos`. If found, the element
/// at `pos` came from position `k`, so recurse. If not found, the element is
/// still at its original position.
fn trace_label(start: usize, n: usize, j_packed: u32) -> usize {
    let mut pos = start;
    loop {
        let mut found = false;
        for k in (pos + 1)..n {
            let j_k = ((j_packed >> (3 * (k - 1))) & 7) as usize;
            if j_k == pos {
                pos = k;
                found = true;
                break;
            }
        }
        if !found {
            return pos;
        }
    }
}
