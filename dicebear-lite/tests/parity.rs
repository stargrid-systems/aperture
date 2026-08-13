use std::fs;
use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The (slug, seed) pairs covered by `tests/fixtures`. Kept inline so the test
/// depends on no extra crates.
fn seeds() -> Vec<(&'static str, String)> {
    let literal: &[(&str, &str)] = &[
        ("empty", ""),
        ("n0", "0"),
        ("n1", "1"),
        ("n2", "2"),
        ("n3", "3"),
        ("n7", "7"),
        ("n42", "42"),
        ("n100", "100"),
        ("n9999", "9999"),
        ("test", "test"),
        ("alice", "alice"),
        ("bob", "bob"),
        ("admin", "admin"),
        ("simon", "Simon"),
        ("capitalA", "A"),
        ("zzz", "zzz"),
        ("hex", "0x1F"),
        ("space", "a b c"),
        ("unicode", "日本語"),
        ("emoji", "🎉"),
        ("mixed", "User-12345_Admin"),
        ("neg", "-5"),
        ("floatish", "3.14"),
    ];
    let mut all: Vec<(&'static str, String)> = literal
        .iter()
        .map(|(slug, seed)| (*slug, (*seed).to_owned()))
        .collect();
    all.push(("longseed", "x".repeat(300)));
    all
}

/// Renders every fixture seed and asserts byte-identical output to the
/// committed `DiceBear` reference SVGs.
#[test]
fn byte_parity_with_dicebear() {
    let mut failures: Vec<(String, usize, usize)> = Vec::new();
    for (slug, seed) in seeds() {
        let expected =
            fs::read_to_string(manifest_dir().join(format!("tests/fixtures/{slug}.svg"))).unwrap();
        let actual = dicebear_lite::Avatar::new(&seed).to_string();
        if actual != expected {
            // Persist the actual output so the mismatch can be diffed locally.
            let _ = fs::write(
                manifest_dir().join(format!("tests/fixtures/{slug}.actual.svg")),
                &actual,
            );
            failures.push((slug.to_owned(), expected.len(), actual.len()));
        }
    }

    assert!(
        failures.is_empty(),
        "parity failures (expected, actual lengths): {failures:#?}"
    );
}

#[test]
fn deterministic_for_same_seed() {
    assert_eq!(
        dicebear_lite::Avatar::new("alice").to_string(),
        dicebear_lite::Avatar::new("alice").to_string()
    );
}

#[test]
fn distinct_seeds_differ() {
    assert_ne!(
        dicebear_lite::Avatar::new("1").to_string(),
        dicebear_lite::Avatar::new("2").to_string()
    );
}
