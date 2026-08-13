// Constellation style data, bootstrapped from DiceBear 10.x (CC0 1.0).
// Committed as plain Rust so the crate parses no JSON. Weight-0 animation
// variants are omitted (never selected); color palettes are pre-sorted to
// match the PRNG shuffle's sort step. Byte parity is verified by
// tests/fixtures.

use crate::Style;
use crate::data::{AttrVal, ComponentDef, Node, VariantDef};

static COMP_COMET: ComponentDef = ComponentDef {
    width: Some(100.0),
    height: Some(100.0),
    probability: Some(22.0),
    translate: Some(((-12.0, 12.0), (-5.0, 8.0))),
    rotate: Some((-12.0, 12.0)),
    scale: None,
    extends: None,
    variants: &[
        (
            "long",
            VariantDef {
                weight: 1.0,
                elements: &[Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbca-comet"))],
                    children: &[
                        Node::El {
                            name: "defs",
                            attrs: &[],
                            children: &[Node::El {
                                name: "linearGradient",
                                attrs: &[
                                    ("id", AttrVal::Lit("dicebearConstellation-long")),
                                    ("gradientUnits", AttrVal::Lit("userSpaceOnUse")),
                                    ("x1", AttrVal::Lit("78")),
                                    ("y1", AttrVal::Lit("12")),
                                    ("x2", AttrVal::Lit("36")),
                                    ("y2", AttrVal::Lit("34")),
                                ],
                                children: &[
                                    Node::El {
                                        name: "stop",
                                        attrs: &[
                                            ("offset", AttrVal::Lit("0")),
                                            ("stop-color", AttrVal::Lit("#fff")),
                                            ("stop-opacity", AttrVal::Lit("0")),
                                        ],
                                        children: &[],
                                    },
                                    Node::El {
                                        name: "stop",
                                        attrs: &[
                                            ("offset", AttrVal::Lit("1")),
                                            ("stop-color", AttrVal::Lit("#fff")),
                                            ("stop-opacity", AttrVal::Lit(".7")),
                                        ],
                                        children: &[],
                                    },
                                ],
                            }],
                        },
                        Node::El {
                            name: "path",
                            attrs: &[
                                ("d", AttrVal::Lit("M78 12 35.3 32.7l1.4 2.6Z")),
                                ("fill", AttrVal::Lit("url(#dicebearConstellation-long)")),
                            ],
                            children: &[],
                        },
                        Node::El {
                            name: "circle",
                            attrs: &[
                                ("cx", AttrVal::Lit("36")),
                                ("cy", AttrVal::Lit("34")),
                                ("r", AttrVal::Lit("1.6")),
                                ("fill", AttrVal::Lit("#fff")),
                                ("fill-opacity", AttrVal::Lit(".9")),
                            ],
                            children: &[],
                        },
                    ],
                }],
            },
        ),
        (
            "short",
            VariantDef {
                weight: 1.0,
                elements: &[Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbca-comet"))],
                    children: &[
                        Node::El {
                            name: "defs",
                            attrs: &[],
                            children: &[Node::El {
                                name: "linearGradient",
                                attrs: &[
                                    ("id", AttrVal::Lit("dicebearConstellation-short")),
                                    ("gradientUnits", AttrVal::Lit("userSpaceOnUse")),
                                    ("x1", AttrVal::Lit("84")),
                                    ("y1", AttrVal::Lit("10")),
                                    ("x2", AttrVal::Lit("60")),
                                    ("y2", AttrVal::Lit("20")),
                                ],
                                children: &[
                                    Node::El {
                                        name: "stop",
                                        attrs: &[
                                            ("offset", AttrVal::Lit("0")),
                                            ("stop-color", AttrVal::Lit("#fff")),
                                            ("stop-opacity", AttrVal::Lit("0")),
                                        ],
                                        children: &[],
                                    },
                                    Node::El {
                                        name: "stop",
                                        attrs: &[
                                            ("offset", AttrVal::Lit("1")),
                                            ("stop-color", AttrVal::Lit("#fff")),
                                            ("stop-opacity", AttrVal::Lit(".7")),
                                        ],
                                        children: &[],
                                    },
                                ],
                            }],
                        },
                        Node::El {
                            name: "path",
                            attrs: &[
                                ("d", AttrVal::Lit("m84 10-24.5 8.9 1 2.2Z")),
                                ("fill", AttrVal::Lit("url(#dicebearConstellation-short)")),
                            ],
                            children: &[],
                        },
                        Node::El {
                            name: "circle",
                            attrs: &[
                                ("cx", AttrVal::Lit("60")),
                                ("cy", AttrVal::Lit("20")),
                                ("r", AttrVal::Lit("1.3")),
                                ("fill", AttrVal::Lit("#fff")),
                                ("fill-opacity", AttrVal::Lit(".9")),
                            ],
                            children: &[],
                        },
                    ],
                }],
            },
        ),
    ],
};

static COMP_CONSTELLATION: ComponentDef = ComponentDef {
    width: Some(100.0),
    height: Some(100.0),
    probability: None,
    translate: Some(((-6.0, 6.0), (-6.0, 6.0))),
    rotate: Some((-180.0, 180.0)),
    scale: None,
    extends: None,
    variants: &[
        (
            "andromeda",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m71.2 63.9-13.9-2.6m0 0-12.8-9.5M24.7 35.9l19.8 15.9m0 0 \
                                     5.3-5.7m0 0 2.6-5.1",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("71.2")),
                            ("cy", AttrVal::Lit("63.9")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("71.2")),
                            ("cy", AttrVal::Lit("63.9")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("57.3")),
                            ("cy", AttrVal::Lit("61.3")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("52.4")),
                            ("cy", AttrVal::Lit("41")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("49.8")),
                            ("cy", AttrVal::Lit("46.1")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("44.5")),
                            ("cy", AttrVal::Lit("51.8")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("24.7")),
                            ("cy", AttrVal::Lit("35.9")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "aquila",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m38.8 48.9 2.5-5.4m0 0 2.5-3.8m-2.5 3.8 13.8 12.7m0 0-14.8 \
                                     4.7M29.7 65l10.6-4.1m14.8-4.7 10.8-24m0 0 3.2-2.8m-14 26.8 \
                                     10.9 18",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("69.1")),
                            ("cy", AttrVal::Lit("29.4")),
                            ("r", AttrVal::Lit("1.3")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("65.9")),
                            ("cy", AttrVal::Lit("32.2")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("66")),
                            ("cy", AttrVal::Lit("74.2")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("55.1")),
                            ("cy", AttrVal::Lit("56.2")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("43.8")),
                            ("cy", AttrVal::Lit("39.7")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.3")),
                            ("cy", AttrVal::Lit("43.5")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.3")),
                            ("cy", AttrVal::Lit("43.5")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("40.3")),
                            ("cy", AttrVal::Lit("60.9")),
                            ("r", AttrVal::Lit("1.3")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("38.8")),
                            ("cy", AttrVal::Lit("48.9")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("29.7")),
                            ("cy", AttrVal::Lit("65")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "auriga",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m31.1 53.3 2.1-22.2m0 0 21.7-2.2m0 0L63 42.6m0 0 4.8 \
                                     22.5M50.1 79l17.7-13.9M50.1 79l-19-25.7",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("67.8")),
                            ("cy", AttrVal::Lit("65.1")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("63")),
                            ("cy", AttrVal::Lit("42.6")),
                            ("r", AttrVal::Lit("1.2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("54.9")),
                            ("cy", AttrVal::Lit("28.9")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("54.9")),
                            ("cy", AttrVal::Lit("28.9")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("50.1")),
                            ("cy", AttrVal::Lit("79")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("33.2")),
                            ("cy", AttrVal::Lit("31.1")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("31.1")),
                            ("cy", AttrVal::Lit("53.3")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "bear",
            VariantDef {
                weight: 0.5,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "M22.1 51.3 36.2 45m0 0 11.3-6.4m0 0 7.7-4.3m0 0 6.3-7m0 0 \
                                     6.4 9.3m0 0 9 10m0 0L70.2 68m0 0-16 10.7m0 0-20-10m0 \
                                     0-6.4-10.4m0 0-5.7-7",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("22.1")),
                            ("cy", AttrVal::Lit("51.3")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("22.1")),
                            ("cy", AttrVal::Lit("51.3")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("36.2")),
                            ("cy", AttrVal::Lit("45")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("47.5")),
                            ("cy", AttrVal::Lit("38.6")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("55.2")),
                            ("cy", AttrVal::Lit("34.3")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("61.5")),
                            ("cy", AttrVal::Lit("27.3")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("67.9")),
                            ("cy", AttrVal::Lit("36.6")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("76.9")),
                            ("cy", AttrVal::Lit("46.6")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("70.2")),
                            ("cy", AttrVal::Lit("68")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("54.2")),
                            ("cy", AttrVal::Lit("78.7")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("34.2")),
                            ("cy", AttrVal::Lit("68.7")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("27.8")),
                            ("cy", AttrVal::Lit("58.3")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("46.2")),
                            ("cy", AttrVal::Lit("46.6")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "bigDipper",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m68.7 36.7 3.7 10.2m0 0-13.6 8.2m0 0-6.5-6.2m0 0 \
                                     16.4-12.2M52.3 48.9l-10.6 1.7m0 0-8.6.7m0 0-10.2 9.1",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("72.4")),
                            ("cy", AttrVal::Lit("46.9")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("68.7")),
                            ("cy", AttrVal::Lit("36.7")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("58.8")),
                            ("cy", AttrVal::Lit("55.1")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("52.3")),
                            ("cy", AttrVal::Lit("48.9")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.7")),
                            ("cy", AttrVal::Lit("50.6")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.7")),
                            ("cy", AttrVal::Lit("50.6")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("33.1")),
                            ("cy", AttrVal::Lit("51.3")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("22.9")),
                            ("cy", AttrVal::Lit("60.4")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "bootes",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m45.1 73.7 11.4-10.4m0 0L44.1 48.8m0 0L32.4 36.4m0 0 \
                                     6.2-13.1m0 0 10.8 4.6m0 0 .1 14.9m0 0 7 20.5m0 0 9.4 1.2m0 0 \
                                     2.7 4.8",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("68.6")),
                            ("cy", AttrVal::Lit("69.3")),
                            ("r", AttrVal::Lit("1.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("65.9")),
                            ("cy", AttrVal::Lit("64.5")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("56.5")),
                            ("cy", AttrVal::Lit("63.3")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("56.5")),
                            ("cy", AttrVal::Lit("63.3")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("49.5")),
                            ("cy", AttrVal::Lit("42.8")),
                            ("r", AttrVal::Lit("1.2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("49.4")),
                            ("cy", AttrVal::Lit("27.9")),
                            ("r", AttrVal::Lit("1.3")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("45.1")),
                            ("cy", AttrVal::Lit("73.7")),
                            ("r", AttrVal::Lit("1.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("44.1")),
                            ("cy", AttrVal::Lit("48.8")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("38.6")),
                            ("cy", AttrVal::Lit("23.3")),
                            ("r", AttrVal::Lit("1.2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("32.4")),
                            ("cy", AttrVal::Lit("36.4")),
                            ("r", AttrVal::Lit("1.2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "camelopardalis",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m62.1 69.1-8.9-6.7m0 0-13.9-8.7m22.8 15.4-8.5-26.5m0 0L39.3 \
                                     53.7m14.3-11.1L41.8 22.2",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("62.1")),
                            ("cy", AttrVal::Lit("69.1")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("62.1")),
                            ("cy", AttrVal::Lit("69.1")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("53.6")),
                            ("cy", AttrVal::Lit("42.6")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("53.2")),
                            ("cy", AttrVal::Lit("62.4")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("39.3")),
                            ("cy", AttrVal::Lit("53.7")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.8")),
                            ("cy", AttrVal::Lit("22.2")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "cassiopeia",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m25.7 34.2 11.2 15.7m0 0 14.4-1.3m0 0 9.1 16.3m0 0 15.3-12.5",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("75.7")),
                            ("cy", AttrVal::Lit("52.4")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("60.4")),
                            ("cy", AttrVal::Lit("64.9")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("51.3")),
                            ("cy", AttrVal::Lit("48.6")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("51.3")),
                            ("cy", AttrVal::Lit("48.6")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("36.9")),
                            ("cy", AttrVal::Lit("49.9")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("25.7")),
                            ("cy", AttrVal::Lit("34.2")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "cepheus",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m49.4 72.2-9.1-20m0 0L58 42m0 0 5.8 18.3m0 0L49.4 \
                                     72.2m-9.1-20-1.7-28.9m0 0L58 42",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("63.8")),
                            ("cy", AttrVal::Lit("60.3")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("63.8")),
                            ("cy", AttrVal::Lit("60.3")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("58")),
                            ("cy", AttrVal::Lit("42")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("49.4")),
                            ("cy", AttrVal::Lit("72.2")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("40.3")),
                            ("cy", AttrVal::Lit("52.2")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("38.6")),
                            ("cy", AttrVal::Lit("23.3")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "coronaBorealis",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m66 27.3 8 15.2m0 0L64 59.2m0 0-12.3 2.9m0 0-10.5 1.6m0 \
                                     0-12.1-5.8m0 0-5.2-20.6",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("74")),
                            ("cy", AttrVal::Lit("42.5")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("66")),
                            ("cy", AttrVal::Lit("27.3")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("64")),
                            ("cy", AttrVal::Lit("59.2")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("64")),
                            ("cy", AttrVal::Lit("59.2")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("51.7")),
                            ("cy", AttrVal::Lit("62.1")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.2")),
                            ("cy", AttrVal::Lit("63.7")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("29.1")),
                            ("cy", AttrVal::Lit("57.9")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("23.9")),
                            ("cy", AttrVal::Lit("37.3")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "corvus",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m37.1 29.1 2.8 1.6m0 0 17.7 5.4m0 0 6.7 26.8m0 0 1.8 \
                                     11.2m-1.8-11.2L34.9 67m0 0 5-36.3",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("66.1")),
                            ("cy", AttrVal::Lit("74.1")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("64.3")),
                            ("cy", AttrVal::Lit("62.9")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("57.6")),
                            ("cy", AttrVal::Lit("36.1")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("57.6")),
                            ("cy", AttrVal::Lit("36.1")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("39.9")),
                            ("cy", AttrVal::Lit("30.7")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("37.1")),
                            ("cy", AttrVal::Lit("29.1")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("34.9")),
                            ("cy", AttrVal::Lit("67")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "delphinus",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m63.2 75.8-8.4-25.7m0 0-4-10.3m0 0L37.6 38m0 0 6 8.3m0 0 \
                                     11.2 3.8",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("63.2")),
                            ("cy", AttrVal::Lit("75.8")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("54.8")),
                            ("cy", AttrVal::Lit("50.1")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("54.8")),
                            ("cy", AttrVal::Lit("50.1")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("50.8")),
                            ("cy", AttrVal::Lit("39.8")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("43.6")),
                            ("cy", AttrVal::Lit("46.3")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("37.6")),
                            ("cy", AttrVal::Lit("38")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "dice",
            VariantDef {
                weight: 0.5,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            ("d", AttrVal::Lit("M36 36h28m0 0v28m0 0H36m0 0V36")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("36")),
                            ("cy", AttrVal::Lit("36")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("64")),
                            ("cy", AttrVal::Lit("36")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("64")),
                            ("cy", AttrVal::Lit("64")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("36")),
                            ("cy", AttrVal::Lit("64")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("50")),
                            ("cy", AttrVal::Lit("50")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("50")),
                            ("cy", AttrVal::Lit("50")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("43")),
                            ("cy", AttrVal::Lit("43")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("57")),
                            ("cy", AttrVal::Lit("43")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("43")),
                            ("cy", AttrVal::Lit("57")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("57")),
                            ("cy", AttrVal::Lit("57")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "grus",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m35.1 45.8 17.6-.7m0 0 9 9.4m0 0-15.1-.7m0 0-12.7-3.4m0 0 \
                                     1.2-4.6m11.5 8-6.8 15.6m6.8-15.6-2.1 11.5m17.2-10.8 \
                                     2.7-19.1m0 0 6.9-5.1",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("71.3")),
                            ("cy", AttrVal::Lit("30.3")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("64.4")),
                            ("cy", AttrVal::Lit("35.4")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("61.7")),
                            ("cy", AttrVal::Lit("54.5")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("61.7")),
                            ("cy", AttrVal::Lit("54.5")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("52.7")),
                            ("cy", AttrVal::Lit("45.1")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("46.6")),
                            ("cy", AttrVal::Lit("53.8")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("44.5")),
                            ("cy", AttrVal::Lit("65.3")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("39.8")),
                            ("cy", AttrVal::Lit("69.4")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("35.1")),
                            ("cy", AttrVal::Lit("45.8")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("33.9")),
                            ("cy", AttrVal::Lit("50.4")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "lacerta",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m56.1 78.4-8.9-17m0 0L48 47m0 0 2.6-5.5m0 0 .4-8.7m0 0-3.8 \
                                     6.1m0 0L48 47",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("56.1")),
                            ("cy", AttrVal::Lit("78.4")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("51")),
                            ("cy", AttrVal::Lit("32.8")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("50.6")),
                            ("cy", AttrVal::Lit("41.5")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("48")),
                            ("cy", AttrVal::Lit("47")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("47.2")),
                            ("cy", AttrVal::Lit("61.4")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("47.2")),
                            ("cy", AttrVal::Lit("38.9")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("47.2")),
                            ("cy", AttrVal::Lit("38.9")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "lynx",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m35.4 63.8 1.1-3m0 0 3.4-1.6m0 0 1.9-4m0 0 8.6-1.3m0 0 \
                                     11.2-8.5m0 0 2.7-12.8m0 0 5.9-3.4",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("70.2")),
                            ("cy", AttrVal::Lit("29.2")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("64.3")),
                            ("cy", AttrVal::Lit("32.6")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("61.6")),
                            ("cy", AttrVal::Lit("45.4")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("50.4")),
                            ("cy", AttrVal::Lit("53.9")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.8")),
                            ("cy", AttrVal::Lit("55.2")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("39.9")),
                            ("cy", AttrVal::Lit("59.2")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("36.5")),
                            ("cy", AttrVal::Lit("60.8")),
                            ("r", AttrVal::Lit("1.9")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("35.4")),
                            ("cy", AttrVal::Lit("63.8")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("35.4")),
                            ("cy", AttrVal::Lit("63.8")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "lyra",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m68.3 27.5-11.6 9.3m0 0-8.1 32.5m0 0-14.2 4.9m0 0 7.5-32m0 0 \
                                     14.8-5.4",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("68.3")),
                            ("cy", AttrVal::Lit("27.5")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("68.3")),
                            ("cy", AttrVal::Lit("27.5")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("56.7")),
                            ("cy", AttrVal::Lit("36.8")),
                            ("r", AttrVal::Lit("1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("48.6")),
                            ("cy", AttrVal::Lit("69.3")),
                            ("r", AttrVal::Lit("1.2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.9")),
                            ("cy", AttrVal::Lit("42.2")),
                            ("r", AttrVal::Lit("1.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("34.4")),
                            ("cy", AttrVal::Lit("74.2")),
                            ("r", AttrVal::Lit("1.3")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "orion",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "M35.8 24.7 58 28m0 0-5.1 19.7m-17.1-23 10.7 27.8m6.4-4.8-3.1 \
                                     2.7m0 0-3.3 2.1m6.4-4.8 12.9 23.5M46.5 52.5l-5.2 \
                                     23m24.5-4.3-24.5 4.3",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("65.8")),
                            ("cy", AttrVal::Lit("71.2")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("65.8")),
                            ("cy", AttrVal::Lit("71.2")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("58")),
                            ("cy", AttrVal::Lit("28")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("52.9")),
                            ("cy", AttrVal::Lit("47.7")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("49.8")),
                            ("cy", AttrVal::Lit("50.4")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("46.5")),
                            ("cy", AttrVal::Lit("52.5")),
                            ("r", AttrVal::Lit("1.7")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("41.3")),
                            ("cy", AttrVal::Lit("75.5")),
                            ("r", AttrVal::Lit("1.6")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("35.8")),
                            ("cy", AttrVal::Lit("24.7")),
                            ("r", AttrVal::Lit("2")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
        (
            "ursaMinor",
            VariantDef {
                weight: 1.0,
                elements: &[
                    Node::El {
                        name: "path",
                        attrs: &[
                            (
                                "d",
                                AttrVal::Lit(
                                    "m49.4 21-3.5 10.3m0 0-1.5 12.6m0 0L50.2 56m0 0-5.4 5.2m0 0 \
                                     10.5 10.7m0 0 4.8-7.2m0 0L50.2 56",
                                ),
                            ),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit(".8")),
                            ("stroke-linecap", AttrVal::Lit("round")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("49.4")),
                            ("cy", AttrVal::Lit("21")),
                            ("r", AttrVal::Lit("2.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("49.4")),
                            ("cy", AttrVal::Lit("21")),
                            ("r", AttrVal::Lit("4")),
                            ("stroke", AttrVal::Color("constellation")),
                            ("stroke-opacity", AttrVal::Lit(".35")),
                            ("stroke-width", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("60.1")),
                            ("cy", AttrVal::Lit("64.7")),
                            ("r", AttrVal::Lit("2.1")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("55.3")),
                            ("cy", AttrVal::Lit("71.9")),
                            ("r", AttrVal::Lit("1.8")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("50.2")),
                            ("cy", AttrVal::Lit("56")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("44.8")),
                            ("cy", AttrVal::Lit("61.2")),
                            ("r", AttrVal::Lit("1.4")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("44.4")),
                            ("cy", AttrVal::Lit("43.9")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("45.9")),
                            ("cy", AttrVal::Lit("31.3")),
                            ("r", AttrVal::Lit("1.5")),
                            ("fill", AttrVal::Color("constellation")),
                        ],
                        children: &[],
                    },
                ],
            },
        ),
    ],
};

static COMP_STAR: ComponentDef = ComponentDef {
    width: Some(10.0),
    height: Some(10.0),
    probability: Some(85.0),
    translate: Some(((-120.0, 120.0), (-120.0, 120.0))),
    rotate: None,
    scale: None,
    extends: None,
    variants: &[
        (
            "faint",
            VariantDef {
                weight: 3.0,
                elements: &[Node::El {
                    name: "circle",
                    attrs: &[
                        ("cx", AttrVal::Lit("5")),
                        ("cy", AttrVal::Lit("5")),
                        ("r", AttrVal::Lit(".6")),
                        ("fill", AttrVal::Lit("#fff")),
                        ("fill-opacity", AttrVal::Lit(".4")),
                    ],
                    children: &[],
                }],
            },
        ),
        (
            "small",
            VariantDef {
                weight: 3.0,
                elements: &[Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbca-tw-small"))],
                    children: &[Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("5")),
                            ("cy", AttrVal::Lit("5")),
                            ("r", AttrVal::Lit(".8")),
                            ("fill", AttrVal::Lit("#fff")),
                            ("fill-opacity", AttrVal::Lit(".6")),
                        ],
                        children: &[],
                    }],
                }],
            },
        ),
        (
            "medium",
            VariantDef {
                weight: 2.0,
                elements: &[Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbca-tw-medium"))],
                    children: &[Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("5")),
                            ("cy", AttrVal::Lit("5")),
                            ("r", AttrVal::Lit("1")),
                            ("fill", AttrVal::Lit("#fff")),
                            ("fill-opacity", AttrVal::Lit(".7")),
                        ],
                        children: &[],
                    }],
                }],
            },
        ),
    ],
};

static COMP_STAR02: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR03: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR04: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR05: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR06: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR07: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR08: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR09: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR10: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR11: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR12: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR13: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR14: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR15: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_STAR16: ComponentDef = ComponentDef {
    width: None,
    height: None,
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: Some("star"),
    variants: &[],
};

static COMP_ANIMATION: ComponentDef = ComponentDef {
    width: Some(100.0),
    height: Some(100.0),
    probability: None,
    translate: None,
    rotate: None,
    scale: None,
    extends: None,
    variants: &[(
        "none",
        VariantDef {
            weight: 1.0,
            elements: &[],
        },
    )],
};

static COMPONENTS: &[(&str, &ComponentDef)] = &[
    ("comet", &COMP_COMET),
    ("constellation", &COMP_CONSTELLATION),
    ("star", &COMP_STAR),
    ("star02", &COMP_STAR02),
    ("star03", &COMP_STAR03),
    ("star04", &COMP_STAR04),
    ("star05", &COMP_STAR05),
    ("star06", &COMP_STAR06),
    ("star07", &COMP_STAR07),
    ("star08", &COMP_STAR08),
    ("star09", &COMP_STAR09),
    ("star10", &COMP_STAR10),
    ("star11", &COMP_STAR11),
    ("star12", &COMP_STAR12),
    ("star13", &COMP_STAR13),
    ("star14", &COMP_STAR14),
    ("star15", &COMP_STAR15),
    ("star16", &COMP_STAR16),
    ("animation", &COMP_ANIMATION),
];

static CANVAS: &[Node] = &[
    Node::Component {
        name: "star",
        attrs: &[("transform", AttrVal::Lit("translate(7.5 7.5)"))],
    },
    Node::Component {
        name: "star02",
        attrs: &[("transform", AttrVal::Lit("translate(32.5 7.5)"))],
    },
    Node::Component {
        name: "star03",
        attrs: &[("transform", AttrVal::Lit("translate(57.5 7.5)"))],
    },
    Node::Component {
        name: "star04",
        attrs: &[("transform", AttrVal::Lit("translate(82.5 7.5)"))],
    },
    Node::Component {
        name: "star05",
        attrs: &[("transform", AttrVal::Lit("translate(7.5 32.5)"))],
    },
    Node::Component {
        name: "star06",
        attrs: &[("transform", AttrVal::Lit("translate(32.5 32.5)"))],
    },
    Node::Component {
        name: "star07",
        attrs: &[("transform", AttrVal::Lit("translate(57.5 32.5)"))],
    },
    Node::Component {
        name: "star08",
        attrs: &[("transform", AttrVal::Lit("translate(82.5 32.5)"))],
    },
    Node::Component {
        name: "star09",
        attrs: &[("transform", AttrVal::Lit("translate(7.5 57.5)"))],
    },
    Node::Component {
        name: "star10",
        attrs: &[("transform", AttrVal::Lit("translate(32.5 57.5)"))],
    },
    Node::Component {
        name: "star11",
        attrs: &[("transform", AttrVal::Lit("translate(57.5 57.5)"))],
    },
    Node::Component {
        name: "star12",
        attrs: &[("transform", AttrVal::Lit("translate(82.5 57.5)"))],
    },
    Node::Component {
        name: "star13",
        attrs: &[("transform", AttrVal::Lit("translate(7.5 82.5)"))],
    },
    Node::Component {
        name: "star14",
        attrs: &[("transform", AttrVal::Lit("translate(32.5 82.5)"))],
    },
    Node::Component {
        name: "star15",
        attrs: &[("transform", AttrVal::Lit("translate(57.5 82.5)"))],
    },
    Node::Component {
        name: "star16",
        attrs: &[("transform", AttrVal::Lit("translate(82.5 82.5)"))],
    },
    Node::Component {
        name: "constellation",
        attrs: &[],
    },
    Node::Component {
        name: "comet",
        attrs: &[],
    },
    Node::Component {
        name: "animation",
        attrs: &[],
    },
];

static BG_COLORS: &[&str] = &[
    "#032729", "#032933", "#0d1e2f", "#131e37", "#181a22", "#1f2040", "#27193c", "#2f182d",
];
static CON_COLORS: &[&str] = &["#c1e0f0", "#e7ecf0", "#e9dab2", "#ece8dd", "#f1d7d2"];

static PALETTES: &[(&str, &[&str])] = &[("background", BG_COLORS), ("constellation", CON_COLORS)];

const METADATA: &str = r#"<metadata xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><rdf:RDF><rdf:Description><dc:title>Constellation</dc:title><dc:creator>DiceBear</dc:creator><dc:source xsi:type="dcterms:URI">https://www.dicebear.com</dc:source><dcterms:license xsi:type="dcterms:URI">https://creativecommons.org/publicdomain/zero/1.0/</dcterms:license><dc:rights>“Constellation” (https://www.dicebear.com) by “DiceBear”, licensed under “CC0 1.0” (https://creativecommons.org/publicdomain/zero/1.0/)</dc:rights></rdf:Description></rdf:RDF></metadata>"#;

pub static CONSTELLATION: Style = Style {
    source_name: "Constellation",
    metadata: METADATA,
    canvas_w: 100.0,
    canvas_h: 100.0,
    canvas: CANVAS,
    components: COMPONENTS,
    palettes: PALETTES,
};
