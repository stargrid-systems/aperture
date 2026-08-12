use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Renders every fixture seed and asserts byte-identical output to the
/// committed `DiceBear` reference SVGs.
#[test]
fn byte_parity_with_dicebear() {
    let manifest: HashMap<String, String> =
        serde_json::from_str(&fs::read_to_string(manifest_dir().join("tests/seeds.json")).unwrap())
            .unwrap();

    let mut failures: Vec<(String, usize, usize)> = Vec::new();
    for (slug, seed) in &manifest {
        let expected =
            fs::read_to_string(manifest_dir().join(format!("tests/fixtures/{slug}.svg"))).unwrap();
        let actual = dicebear_lite::constellation(seed);
        if actual != expected {
            // Persist the actual output so the mismatch can be diffed locally.
            let _ = fs::write(
                manifest_dir().join(format!("tests/fixtures/{slug}.actual.svg")),
                &actual,
            );
            failures.push((slug.clone(), expected.len(), actual.len()));
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
        dicebear_lite::constellation("alice"),
        dicebear_lite::constellation("alice")
    );
}

#[test]
fn distinct_seeds_differ() {
    assert_ne!(
        dicebear_lite::constellation("1"),
        dicebear_lite::constellation("2")
    );
}
