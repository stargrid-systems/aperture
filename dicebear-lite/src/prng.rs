//! `DiceBear`'s key-based PRNG: FNV-1a over UTF-16 code units seeding
//! Mulberry32.
//!
//! Every draw is derived independently from `(seed, key)`, so the call order is
//! irrelevant. This is a direct port of `@dicebear/prng`.

use crate::number::{equals, floor_index, math_round};

const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// 32-bit FNV-1a hash over the UTF-16 code units of `input`.
pub fn fnv1a_hash(input: &str) -> u32 {
    let mut hash = FNV_OFFSET;
    for unit in input.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// FNV-1a hash as an 8-character lowercase hex string.
pub fn fnv1a_hex(input: &str) -> String {
    format!("{:08x}", fnv1a_hash(input))
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

/// Key-based pseudorandom value generator.
pub struct Prng {
    seed: String,
}

impl Prng {
    pub fn new(seed: &str) -> Self {
        Self {
            seed: seed.to_owned(),
        }
    }

    /// A single float in `[0, 1)` derived from `(seed, key)`.
    fn value(&self, key: &str) -> f64 {
        let hashed = fnv1a_hash(&format!("{}:{key}", self.seed));
        Mulberry32::new(hashed).next_float()
    }

    pub fn bool(&self, key: &str, likelihood: f64) -> bool {
        self.value(key) * 100.0 < likelihood
    }

    pub fn float(&self, key: &str, min: f64, max: f64) -> f64 {
        math_round(self.value(key).mul_add(max - min, min) * 10000.0) / 10000.0
    }

    /// Picks one item. The pool is sorted by UTF-16 code units before drawing,
    /// matching `Prng.pick`. Constellation only ever passes empty pools here.
    pub fn pick<'a>(&self, key: &str, items: &[&'a str]) -> Option<&'a str> {
        if items.is_empty() {
            return None;
        }
        if items.len() == 1 {
            return Some(items[0]);
        }
        let mut sorted: Vec<&str> = items.to_vec();
        sorted.sort_unstable();
        let index = floor_index(self.value(key), sorted.len());
        Some(sorted[index.min(sorted.len() - 1)])
    }

    /// Weighted pick over `(name, weight)` pairs, sorted by name.
    pub fn weighted_pick(&self, key: &str, mut entries: Vec<(String, f64)>) -> String {
        if entries.is_empty() {
            return String::new();
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let total: f64 = entries.iter().map(|(_, w)| *w).sum();
        if equals(total, 0.0) {
            let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
            return self.pick(key, &names).unwrap_or("").to_owned();
        }
        let threshold = self.value(key) * total;
        let mut cumulative = 0.0;
        for (name, weight) in &entries {
            cumulative += *weight;
            if threshold < cumulative {
                return name.clone();
            }
        }
        entries.last().unwrap().0.clone()
    }

    /// Fisher-Yates shuffle keyed by `key`. The pool is sorted before
    /// shuffling.
    pub fn shuffle(&self, key: &str, mut items: Vec<String>) -> Vec<String> {
        items.sort();
        let mut rng = Mulberry32::new(fnv1a_hash(&format!("{}:{key}", self.seed)));
        let mut i = items.len();
        while i > 1 {
            i -= 1;
            let j = floor_index(rng.next_float(), i + 1);
            items.swap(i, j);
        }
        items
    }
}
