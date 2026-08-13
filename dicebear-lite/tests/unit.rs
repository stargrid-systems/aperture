use dicebear_lite::internal::{Escaped, Num, Prng, hash_u32};

#[test]
fn num_formats_integers() {
    assert_eq!(Num(0.0).to_string(), "0");
    assert_eq!(Num(100.0).to_string(), "100");
    assert_eq!(Num(-12.0).to_string(), "-12");
}

#[test]
fn num_formats_decimals() {
    assert_eq!(Num(2.5).to_string(), "2.5");
    assert_eq!(Num(7.5).to_string(), "7.5");
}

#[test]
fn num_trims_trailing_zeros() {
    assert_eq!(Num(1.0).to_string(), "1");
    assert_eq!(Num(1.50).to_string(), "1.5");
}

#[test]
fn hash_u32_is_deterministic() {
    assert_eq!(hash_u32("test", "seed1"), hash_u32("test", "seed1"));
    assert_ne!(hash_u32("test", "seed1"), hash_u32("test", "seed2"));
}

#[test]
fn shuffle_zero_stays_in_bounds() {
    let prng = Prng::new("test");
    for n in 1..=8 {
        let idx = prng.shuffle_zero(&["test", "shuffle"], n);
        assert!(idx < n, "idx={idx} n={n}");
    }
}

#[test]
fn shuffle_zero_is_deterministic() {
    let prng = Prng::new("test");
    let a = prng.shuffle_zero(&["color"], 5);
    let b = prng.shuffle_zero(&["color"], 5);
    assert_eq!(a, b);
}

#[test]
fn escaped_handles_special_chars() {
    assert_eq!(Escaped("a&b").to_string(), "a&amp;b");
    assert_eq!(Escaped("<tag>").to_string(), "&lt;tag&gt;");
    assert_eq!(Escaped("\"q\"").to_string(), "&quot;q&quot;");
    assert_eq!(Escaped("it's").to_string(), "it&apos;s");
}

#[test]
fn escaped_passes_through_normal_text() {
    assert_eq!(Escaped("normal").to_string(), "normal");
    assert_eq!(Escaped("").to_string(), "");
}
