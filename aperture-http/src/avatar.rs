//! Deterministic constellation-style avatars.
//!
//! Each actor gets a stable, abstract starfield derived from its id. The art is
//! generated locally so the gateway stays fully offline and the frontend only
//! has to render an `<img>` URL. The look is inspired by `DiceBear`'s
//! "constellation" style rather than a byte-for-byte port.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use aperture_storage::ActorId;

/// Edge length of the square SVG viewport. Avatars are clipped to a circle by
/// the consumer, so the background fills the whole viewport.
const VIEW: f32 = 64.0;

/// Inset keeping stars away from the rounded edge so they are never cut off.
const MARGIN: f32 = 10.0;

/// Deep-space background gradients, indexed by the RNG.
const BACKGROUNDS: &[(&str, &str)] = &[
    ("#0b1026", "#1a1b3a"),
    ("#0d1b2a", "#16314a"),
    ("#170b2e", "#2a164a"),
    ("#0a1a1f", "#143a40"),
    ("#1a0b14", "#3a1430"),
];

/// Star tints, applied to both specks and constellation stars.
const STAR_COLORS: &[&str] = &["#ffffff", "#cfe0ff", "#ffe9c4", "#e2c4ff", "#bff0ff"];

/// Builds the absolute API path for an actor's avatar.
pub fn avatar_url(actor_id: ActorId) -> String {
    format!("/api/v1/avatars/{actor_id}")
}

/// Renders the avatar for `actor_id` as an SVG document.
pub fn render_svg(actor_id: ActorId) -> String {
    let mut rng = Rng::new(actor_id.get().unsigned_abs());
    let (bg_from, bg_to) = BACKGROUNDS[rng.usize_below(BACKGROUNDS.len())];

    let mut out = String::with_capacity(1024);
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {VIEW:.0} {VIEW:.0}\" \
         width=\"128\" height=\"128\">"
    );

    out.push_str("<defs>");
    let _ = write!(
        out,
        "<linearGradient id=\"bg\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"><stop offset=\"0\" \
         stop-color=\"{bg_from}\"/><stop offset=\"1\" stop-color=\"{bg_to}\"/></linearGradient>"
    );
    let _ = write!(
        out,
        "<radialGradient id=\"glow\"><stop offset=\"0\" stop-color=\"#ffffff\" \
         stop-opacity=\"0.85\"/><stop offset=\"1\" stop-color=\"#ffffff\" \
         stop-opacity=\"0\"/></radialGradient>"
    );
    out.push_str("</defs>");

    let _ = write!(
        out,
        "<rect width=\"{VIEW:.0}\" height=\"{VIEW:.0}\" fill=\"url(#bg)\"/>"
    );

    render_specks(&mut out, &mut rng);
    let stars = generate_stars(&mut rng);
    render_links(&mut out, &stars);
    render_stars(&mut out, &stars);

    out.push_str("</svg>");
    out
}

/// A single constellation point.
struct Star {
    x: f32,
    y: f32,
    radius: f32,
    color: &'static str,
    bright: bool,
}

/// Scatters many tiny dim stars across the background for depth.
fn render_specks(out: &mut String, rng: &mut Rng) {
    let count = 14 + rng.usize_below(8);
    for _ in 0..count {
        let x = rng.between(0.0, VIEW);
        let y = rng.between(0.0, VIEW);
        let radius = rng.between(0.3, 0.7);
        let opacity = rng.between(0.25, 0.6);
        let _ = write!(
            out,
            "<circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"{radius:.2}\" fill=\"#ffffff\" \
             fill-opacity=\"{opacity:.2}\"/>"
        );
    }
}

/// Generates the handful of bright, linked stars that form the constellation.
fn generate_stars(rng: &mut Rng) -> Vec<Star> {
    let count = 6 + rng.usize_below(4);
    let bright_count = 2 + rng.usize_below(2);
    let bright_indices = pick_distinct(rng, count, bright_count);
    let mut stars = Vec::with_capacity(count);
    for i in 0..count {
        let bright = bright_indices.contains(&i);
        let radius = if bright {
            rng.between(1.8, 2.6)
        } else {
            rng.between(0.9, 1.5)
        };
        stars.push(Star {
            x: rng.between(MARGIN, VIEW - MARGIN),
            y: rng.between(MARGIN, VIEW - MARGIN),
            radius,
            color: STAR_COLORS[rng.usize_below(STAR_COLORS.len())],
            bright,
        });
    }
    stars
}

/// Draws a faint line from each star to its nearest neighbor, deduplicated.
fn render_links(out: &mut String, stars: &[Star]) {
    let mut drawn = BTreeSet::new();
    for (i, a) in stars.iter().enumerate() {
        let Some((j, _)) = stars
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(j, b)| (j, sq_dist(a, b)))
            .min_by(|(_, d1), (_, d2)| d1.total_cmp(d2))
        else {
            continue;
        };
        let key = if i < j { (i, j) } else { (j, i) };
        if drawn.insert(key) {
            let b = &stars[j];
            let _ = write!(
                out,
                "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#ffffff\" \
                 stroke-opacity=\"0.22\" stroke-width=\"0.5\"/>",
                a.x, a.y, b.x, b.y
            );
        }
    }
}

/// Draws glows for bright stars, then every star on top.
fn render_stars(out: &mut String, stars: &[Star]) {
    for star in stars {
        if star.bright {
            let glow = star.radius * 2.6;
            let _ = write!(
                out,
                "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{glow:.2}\" fill=\"url(#glow)\"/>",
                star.x, star.y
            );
        }
    }
    for star in stars {
        let _ = write!(
            out,
            "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"{}\"/>",
            star.x, star.y, star.radius, star.color
        );
    }
}

/// Squared Euclidean distance between two stars.
fn sq_dist(a: &Star, b: &Star) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx.mul_add(dx, dy * dy)
}

/// Returns `count` distinct indices in `0..n` using a partial Fisher-Yates.
fn pick_distinct(rng: &mut Rng, n: usize, count: usize) -> Vec<usize> {
    let take = count.min(n);
    let mut indices: Vec<usize> = (0..n).collect();
    for k in 0..take {
        let swap_with = k + rng.usize_below(n - k);
        indices.swap(k, swap_with);
    }
    indices.truncate(take);
    indices
}

/// `SplitMix64` PRNG. Good distribution for any seed, including zero.
struct Rng(u64);

impl Rng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    const fn next_u32(&mut self) -> u32 {
        let bytes = self.next_u64().to_ne_bytes();
        u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Uniform float in `[0, 1)` built from random mantissa bits.
    const fn unit(&mut self) -> f32 {
        let bits = (self.next_u32() >> 9) | 0x3F80_0000;
        f32::from_bits(bits) - 1.0
    }

    fn between(&mut self, lo: f32, hi: f32) -> f32 {
        self.unit().mul_add(hi - lo, lo)
    }

    const fn usize_below(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        (self.next_u32() as usize) % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_url_is_stable_and_formatted() {
        assert_eq!(avatar_url(ActorId::from_i64(7)), "/api/v1/avatars/7");
    }

    #[test]
    fn render_is_deterministic() {
        let a = render_svg(ActorId::from_i64(42));
        let b = render_svg(ActorId::from_i64(42));
        assert_eq!(a, b);
    }

    #[test]
    fn distinct_actors_render_distinct_avatars() {
        let a = render_svg(ActorId::from_i64(1));
        let b = render_svg(ActorId::from_i64(2));
        assert_ne!(a, b);
    }

    #[test]
    fn render_emits_well_formed_svg() {
        let svg = render_svg(ActorId::from_i64(0));
        assert!(svg.starts_with("<svg "), "got: {svg}");
        assert!(svg.contains("viewBox=\"0 0 64 64\""));
        assert!(svg.contains("url(#bg)"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn pick_distinct_respects_bounds() {
        let mut rng = Rng::new(123);
        let picked = pick_distinct(&mut rng, 5, 3);
        assert_eq!(picked.len(), 3);
        for &i in &picked {
            assert!(i < 5);
        }
    }
}
