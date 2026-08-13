//! `DiceBear`'s key-based PRNG: FNV-1a over UTF-16 code units seeding
//! Mulberry32.
//!
//! Every draw is derived independently from `(seed, key)`, so the call order is
//! irrelevant. A "key" is several string fragments concatenated; hashing walks
//! them incrementally so no allocation is needed. This is a direct port of
//! `@dicebear/prng`.

use crate::number::{equals, floor_index, math_round};

const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// Stack buffer large enough for the most variants (19) or palette entries (8)
/// any constellation component uses.
const BUF: usize = 32;

#[inline]
fn step(hash: u32, unit: u16) -> u32 {
    (hash ^ u32::from(unit)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a over the UTF-16 code units of `seed`, a `:` separator, then the
/// concatenated `key` fragments.
fn hash_seed_key(seed: &str, key: &[&str]) -> u32 {
    let mut h = FNV_OFFSET;
    for unit in seed.encode_utf16() {
        h = step(h, unit);
    }
    h = step(h, 0x3A);
    for fragment in key {
        for unit in fragment.encode_utf16() {
            h = step(h, unit);
        }
    }
    h
}

/// FNV-1a hash of `prefix + ":" + s` over UTF-16 code units. Used for the
/// per-seed `<defs>` id suffix.
pub fn hash_u32(prefix: &str, s: &str) -> u32 {
    let mut h = FNV_OFFSET;
    for unit in prefix.encode_utf16() {
        h = step(h, unit);
    }
    h = step(h, 0x3A);
    for unit in s.encode_utf16() {
        h = step(h, unit);
    }
    h
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

    fn value(&self, key: &[&str]) -> f64 {
        Mulberry32::new(hash_seed_key(self.seed, key)).next_float()
    }

    pub fn bool(&self, key: &[&str], likelihood: f64) -> bool {
        self.value(key) * 100.0 < likelihood
    }

    pub fn float(&self, key: &[&str], min: f64, max: f64) -> f64 {
        // Two separate roundings, matching JavaScript's `min + value * (max - min)`
        // (JS has no fused multiply-add).
        math_round((min + self.value(key) * (max - min)) * 10000.0) / 10000.0
    }

    /// Weighted pick over `(name, weight)` pairs, sorted by name.
    pub fn weighted_pick(
        &self,
        key: &[&str],
        entries: &[(&'static str, f64)],
    ) -> Option<&'static str> {
        let n = entries.len();
        if n == 0 {
            return None;
        }
        let mut order = [0usize; BUF];
        for (i, slot) in order.iter_mut().enumerate().take(n) {
            *slot = i;
        }
        order[..n].sort_unstable_by(|&a, &b| entries[a].0.cmp(entries[b].0));

        let total: f64 = order[..n].iter().map(|&i| entries[i].1).sum();
        let threshold = if equals(total, 0.0) {
            // All-zero weights: fall back to a uniform pick across the pool.
            return Some(entries[order[floor_index(self.value(key), n)]].0);
        } else {
            self.value(key) * total
        };

        let mut cumulative = 0.0;
        for &i in &order[..n] {
            cumulative += entries[i].1;
            if threshold < cumulative {
                return Some(entries[i].0);
            }
        }
        Some(entries[order[n - 1]].0)
    }

    /// Returns the index that lands at position 0 after a Fisher-Yates shuffle
    /// of `0..n`, matching `Prng.shuffle` (which sorts first; callers pass a
    /// pre-sorted pool).
    pub fn shuffle_zero(&self, key: &[&str], n: usize) -> usize {
        let mut indices = [0usize; BUF];
        for (i, slot) in indices.iter_mut().enumerate().take(n) {
            *slot = i;
        }
        let mut rng = Mulberry32::new(hash_seed_key(self.seed, key));
        let mut i = n;
        while i > 1 {
            i -= 1;
            let j = floor_index(rng.next_float(), i + 1);
            indices.swap(i, j);
        }
        indices[0]
    }
}
