use std::fs;
use std::path::PathBuf;

use dicebear_lite::{Avatar, CONSTELLATION};

fn fixture_path(slug: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{slug}.svg"))
}

/// Renders `seed` and asserts byte-identical output to the committed reference
/// SVG. The `slug` selects the fixture file.
#[track_caller]
fn assert_parity(seed: &str, slug: &str) {
    let expected = fs::read_to_string(fixture_path(slug)).unwrap();
    let actual = Avatar::new(seed, &CONSTELLATION).to_string();
    assert_eq!(actual, expected, "seed={seed:?} (slug={slug})");
}

#[test]
fn empty_seed() {
    assert_parity("", "empty");
}

#[test]
fn single_digit_seeds() {
    assert_parity("0", "n0");
    assert_parity("1", "n1");
    assert_parity("2", "n2");
    assert_parity("3", "n3");
    assert_parity("7", "n7");
}

#[test]
fn multi_digit_seeds() {
    assert_parity("42", "n42");
    assert_parity("100", "n100");
    assert_parity("9999", "n9999");
}

#[test]
fn short_alpha_seeds() {
    assert_parity("test", "test");
    assert_parity("bob", "bob");
    assert_parity("admin", "admin");
    assert_parity("zzz", "zzz");
}

#[test]
fn name_seeds() {
    assert_parity("alice", "alice");
    assert_parity("Simon", "simon");
}

#[test]
fn single_capital_letter() {
    assert_parity("A", "capitalA");
}

#[test]
fn hex_seed() {
    assert_parity("0x1F", "hex");
}

#[test]
fn spaces_in_seed() {
    assert_parity("a b c", "space");
}

#[test]
fn negative_number() {
    assert_parity("-5", "neg");
}

#[test]
fn decimal_number() {
    assert_parity("3.14", "floatish");
}

#[test]
fn mixed_alphanumeric() {
    assert_parity("User-12345_Admin", "mixed");
}

#[test]
fn unicode_seed() {
    assert_parity("日本語", "unicode");
}

#[test]
fn emoji_seed() {
    assert_parity("🎉", "emoji");
}

#[test]
fn long_seed() {
    assert_parity(&"x".repeat(300), "longseed");
}

#[test]
fn deterministic_for_same_seed() {
    assert_eq!(
        Avatar::new("alice", &CONSTELLATION).to_string(),
        Avatar::new("alice", &CONSTELLATION).to_string()
    );
}

#[test]
fn distinct_seeds_differ() {
    assert_ne!(
        Avatar::new("1", &CONSTELLATION).to_string(),
        Avatar::new("2", &CONSTELLATION).to_string()
    );
}
