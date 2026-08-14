use std::sync::LazyLock;

use dicebear_core::{Avatar as OracleAvatar, Style as OracleStyle};
use dicebear_lite::{Animation, Avatar, CONSTELLATION, PLANETS, Speed, THUMBS};
use serde_json::json;

type Case = (
    &'static str,
    &'static dicebear_lite::Style<'static>,
    OracleStyle,
);

static STYLES: LazyLock<[Case; 3]> = LazyLock::new(|| {
    [
        (
            "constellation",
            &CONSTELLATION,
            OracleStyle::from_str(dicebear_styles::CONSTELLATION).unwrap(),
        ),
        (
            "planets",
            &PLANETS,
            OracleStyle::from_str(dicebear_styles::PLANETS).unwrap(),
        ),
        (
            "thumbs",
            &THUMBS,
            OracleStyle::from_str(dicebear_styles::THUMBS).unwrap(),
        ),
    ]
});

const SEEDS: &[&str] = &[
    "",
    "0",
    "1",
    "2",
    "3",
    "7",
    "42",
    "100",
    "9999",
    "test",
    "bob",
    "admin",
    "zzz",
    "alice",
    "Simon",
    "A",
    "0x1F",
    "a b c",
    "-5",
    "3.14",
    "User-12345_Admin",
    "日本語",
    "🎉",
];

const FIXED_SEEDS: &[&str] = &["", "alice", "42", "Simon"];

const SPEEDS: &[Speed] = &[
    Speed::Fastest,
    Speed::Fast,
    Speed::Medium,
    Speed::Slow,
    Speed::Slowest,
];

#[track_caller]
fn assert_parity(
    name: &str,
    lite: &dicebear_lite::Style,
    oracle: &OracleStyle,
    seed: &str,
    animation: Animation,
) {
    let options = match animation {
        Animation::Off => json!({"seed": seed}),
        Animation::Random => json!({"seed": seed, "tags": ["animation"]}),
        Animation::Fixed(variant) => json!({"seed": seed, "animationVariant": variant.as_str()}),
    };
    let expected = OracleAvatar::new(oracle, options)
        .unwrap()
        .to_svg()
        .to_string();
    let actual = Avatar::new(lite, seed).animation(animation).to_string();

    if expected == actual {
        return;
    }

    let pos = expected
        .bytes()
        .zip(actual.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    let exp_len = expected.len();
    let act_len = actual.len();
    let lo = expected.floor_char_boundary(pos.saturating_sub(40));
    let hi = expected.ceil_char_boundary((pos + 40).min(exp_len));
    let exp_snip = &expected[lo..hi];
    let act_snip = &actual[lo..hi];
    panic!(
        "parity mismatch: style={name} seed={seed:?} animation={animation:?} first diff at byte \
         {pos} (exp {exp_len}B act {act_len}B) exp={exp_snip:?} act={act_snip:?}"
    );
}

#[test]
fn parity_off() {
    let long_seed = "x".repeat(300);
    for &(name, lite, ref oracle) in STYLES.iter() {
        for &seed in SEEDS {
            assert_parity(name, lite, oracle, seed, Animation::Off);
        }
        assert_parity(name, lite, oracle, &long_seed, Animation::Off);
    }
}

#[test]
fn parity_random() {
    let long_seed = "x".repeat(300);
    for &(name, lite, ref oracle) in STYLES.iter() {
        for &seed in SEEDS {
            assert_parity(name, lite, oracle, seed, Animation::Random);
        }
        assert_parity(name, lite, oracle, &long_seed, Animation::Random);
    }
}

#[test]
fn parity_fixed() {
    for &(name, lite, ref oracle) in STYLES.iter() {
        for &seed in FIXED_SEEDS {
            for &speed in SPEEDS {
                assert_parity(name, lite, oracle, seed, Animation::Fixed(speed));
            }
        }
    }
}

#[test]
fn deterministic_for_same_seed() {
    let a = Avatar::new(&CONSTELLATION, "alice").to_string();
    let b = Avatar::new(&CONSTELLATION, "alice").to_string();
    assert_eq!(a, b);
}

#[test]
fn distinct_seeds_differ() {
    let a = Avatar::new(&CONSTELLATION, "1").to_string();
    let b = Avatar::new(&CONSTELLATION, "2").to_string();
    assert_ne!(a, b);
}
