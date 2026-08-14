// DiceBear 10.x planets style (CC0 1.0).

use crate::Style;
use crate::data::{
    AttrVal, Canvas, ColorRef, ComponentDef, Node, Palette, Range, VariantDef, Variants,
};

static COMP_MOONS: ComponentDef = ComponentDef {
    name: "moons",
    width: Some(63.0),
    height: Some(50.0),
    probability: Some(65.0),
    translate: Some((Range(-7.9365, 7.9365), Range(-10.0, 10.0))),
    rotate: None,

    variants: Variants::new(&[
        VariantDef {
            name: "one",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-moons"))],
                children: &[Node::El {
                    name: "circle",
                    attrs: &[
                        ("cx", AttrVal::Lit("58")),
                        ("cy", AttrVal::Lit("4")),
                        ("r", AttrVal::Lit("4")),
                        ("fill", AttrVal::Color(MOON)),
                    ],
                    children: &[],
                }],
            }],
        },
        VariantDef {
            name: "tiny",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-moons"))],
                children: &[Node::El {
                    name: "circle",
                    attrs: &[
                        ("cx", AttrVal::Lit("60")),
                        ("cy", AttrVal::Lit("42")),
                        ("r", AttrVal::Lit("2.5")),
                        ("fill", AttrVal::Color(MOON)),
                    ],
                    children: &[],
                }],
            }],
        },
        VariantDef {
            name: "two",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-moons"))],
                children: &[
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("56")),
                            ("cy", AttrVal::Lit("10")),
                            ("r", AttrVal::Lit("3.5")),
                            ("fill", AttrVal::Color(MOON)),
                        ],
                        children: &[],
                    },
                    Node::El {
                        name: "circle",
                        attrs: &[
                            ("cx", AttrVal::Lit("3")),
                            ("cy", AttrVal::Lit("47")),
                            ("r", AttrVal::Lit("2.2")),
                            ("fill", AttrVal::Color(MOON)),
                        ],
                        children: &[],
                    },
                ],
            }],
        },
    ]),
};

static COMP_PLANET: ComponentDef = ComponentDef {
    name: "planet",
    width: Some(60.0),
    height: Some(60.0),
    probability: None,
    translate: None,
    rotate: None,

    variants: Variants::new(&[VariantDef {
        name: "disc",
        weight: 1.0,
        tags: &[],
        elements: &[Node::El {
            name: "circle",
            attrs: &[
                ("cx", AttrVal::Lit("30")),
                ("cy", AttrVal::Lit("30")),
                ("r", AttrVal::Lit("30")),
                ("fill", AttrVal::Color(PLANET)),
            ],
            children: &[],
        }],
    }]),
};

static COMP_RING: ComponentDef = ComponentDef {
    name: "ring",
    width: Some(96.0),
    height: Some(32.0),
    probability: Some(45.0),
    translate: None,
    rotate: Some(Range(-25.0, 25.0)),

    variants: Variants::new(&[
        VariantDef {
            name: "bold",
            weight: 1.0,
            tags: &[],
            elements: &[
                Node::El {
                    name: "defs",
                    attrs: &[],
                    children: &[Node::El {
                        name: "mask",
                        attrs: &[("id", AttrVal::Lit("dicebearPlanets-ring-bold"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("d", AttrVal::Lit("M-12-44h120V76H-12z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("M18 16a30 30 0 0 1 60 0Z")),
                                    ("fill", AttrVal::Lit("#000")),
                                ],
                                children: &[],
                            },
                        ],
                    }],
                },
                Node::El {
                    name: "g",
                    attrs: &[("mask", AttrVal::Lit("url(#dicebearPlanets-ring-bold)"))],
                    children: &[Node::El {
                        name: "ellipse",
                        attrs: &[
                            ("cx", AttrVal::Lit("48")),
                            ("cy", AttrVal::Lit("16")),
                            ("rx", AttrVal::Lit("44")),
                            ("ry", AttrVal::Lit("13.5")),
                            ("stroke", AttrVal::Lit("#fff")),
                            ("stroke-opacity", AttrVal::Lit(".4")),
                            ("stroke-width", AttrVal::Lit("4")),
                        ],
                        children: &[],
                    }],
                },
            ],
        },
        VariantDef {
            name: "double",
            weight: 1.0,
            tags: &[],
            elements: &[
                Node::El {
                    name: "defs",
                    attrs: &[],
                    children: &[Node::El {
                        name: "mask",
                        attrs: &[("id", AttrVal::Lit("dicebearPlanets-ring-double"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("d", AttrVal::Lit("M-12-44h120V76H-12z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("M18 16a30 30 0 0 1 60 0Z")),
                                    ("fill", AttrVal::Lit("#000")),
                                ],
                                children: &[],
                            },
                        ],
                    }],
                },
                Node::El {
                    name: "g",
                    attrs: &[("mask", AttrVal::Lit("url(#dicebearPlanets-ring-double)"))],
                    children: &[
                        Node::El {
                            name: "ellipse",
                            attrs: &[
                                ("cx", AttrVal::Lit("48")),
                                ("cy", AttrVal::Lit("16")),
                                ("rx", AttrVal::Lit("42")),
                                ("ry", AttrVal::Lit("12.5")),
                                ("stroke", AttrVal::Lit("#fff")),
                                ("stroke-opacity", AttrVal::Lit(".5")),
                                ("stroke-width", AttrVal::Lit("1.6")),
                            ],
                            children: &[],
                        },
                        Node::El {
                            name: "ellipse",
                            attrs: &[
                                ("cx", AttrVal::Lit("48")),
                                ("cy", AttrVal::Lit("16")),
                                ("rx", AttrVal::Lit("46.5")),
                                ("ry", AttrVal::Lit("15")),
                                ("stroke", AttrVal::Lit("#fff")),
                                ("stroke-opacity", AttrVal::Lit(".35")),
                                ("stroke-width", AttrVal::Lit("1")),
                            ],
                            children: &[],
                        },
                    ],
                },
            ],
        },
        VariantDef {
            name: "thin",
            weight: 2.0,
            tags: &[],
            elements: &[
                Node::El {
                    name: "defs",
                    attrs: &[],
                    children: &[Node::El {
                        name: "mask",
                        attrs: &[("id", AttrVal::Lit("dicebearPlanets-ring-thin"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("d", AttrVal::Lit("M-12-44h120V76H-12z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("M18 16a30 30 0 0 1 60 0Z")),
                                    ("fill", AttrVal::Lit("#000")),
                                ],
                                children: &[],
                            },
                        ],
                    }],
                },
                Node::El {
                    name: "g",
                    attrs: &[("mask", AttrVal::Lit("url(#dicebearPlanets-ring-thin)"))],
                    children: &[Node::El {
                        name: "ellipse",
                        attrs: &[
                            ("cx", AttrVal::Lit("48")),
                            ("cy", AttrVal::Lit("16")),
                            ("rx", AttrVal::Lit("43")),
                            ("ry", AttrVal::Lit("13")),
                            ("stroke", AttrVal::Lit("#fff")),
                            ("stroke-opacity", AttrVal::Lit(".55")),
                            ("stroke-width", AttrVal::Lit("2")),
                        ],
                        children: &[],
                    }],
                },
            ],
        },
    ]),
};

static COMP_SHADE: ComponentDef = ComponentDef {
    name: "shade",
    width: Some(61.0),
    height: Some(61.0),
    probability: None,
    translate: None,
    rotate: None,

    variants: Variants::new(&[
        VariantDef {
            name: "hard",
            weight: 0.0,
            tags: &[],
            elements: &[
                Node::El {
                    name: "path",
                    attrs: &[
                        (
                            "d",
                            AttrVal::Lit(
                                "M55.2 13.7a30 30 0 0 1-41.5 41.5 33.5 33.5 0 0 0 41.5-41.5",
                            ),
                        ),
                        ("fill", AttrVal::Lit("#000")),
                        ("fill-opacity", AttrVal::Lit(".18")),
                    ],
                    children: &[],
                },
                Node::El {
                    name: "path",
                    attrs: &[
                        (
                            "d",
                            AttrVal::Lit("M4.8 46.3A30 30 0 0 1 46.3 4.8 33.5 33.5 0 0 0 4.8 46.3"),
                        ),
                        ("fill", AttrVal::Lit("#fff")),
                        ("fill-opacity", AttrVal::Lit(".12")),
                    ],
                    children: &[],
                },
            ],
        },
        VariantDef {
            name: "soft",
            weight: 1.0,
            tags: &[],
            elements: &[
                Node::El {
                    name: "defs",
                    attrs: &[],
                    children: &[Node::El {
                        name: "radialGradient",
                        attrs: &[
                            ("id", AttrVal::Lit("dicebearPlanets-shade")),
                            ("gradientUnits", AttrVal::Lit("userSpaceOnUse")),
                            ("cx", AttrVal::Lit("22")),
                            ("cy", AttrVal::Lit("22")),
                            ("r", AttrVal::Lit("44")),
                        ],
                        children: &[
                            Node::El {
                                name: "stop",
                                attrs: &[
                                    ("offset", AttrVal::Lit("0")),
                                    ("stop-color", AttrVal::Lit("#fff")),
                                    ("stop-opacity", AttrVal::Lit(".14")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "stop",
                                attrs: &[
                                    ("offset", AttrVal::Lit(".38")),
                                    ("stop-color", AttrVal::Lit("#fff")),
                                    ("stop-opacity", AttrVal::Lit("0")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "stop",
                                attrs: &[
                                    ("offset", AttrVal::Lit(".55")),
                                    ("stop-color", AttrVal::Lit("#000")),
                                    ("stop-opacity", AttrVal::Lit("0")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "stop",
                                attrs: &[
                                    ("offset", AttrVal::Lit("1")),
                                    ("stop-color", AttrVal::Lit("#000")),
                                    ("stop-opacity", AttrVal::Lit(".26")),
                                ],
                                children: &[],
                            },
                        ],
                    }],
                },
                Node::El {
                    name: "circle",
                    attrs: &[
                        ("cx", AttrVal::Lit("30")),
                        ("cy", AttrVal::Lit("30")),
                        ("r", AttrVal::Lit("30")),
                        ("fill", AttrVal::Lit("url(#dicebearPlanets-shade)")),
                    ],
                    children: &[],
                },
                Node::El {
                    name: "path",
                    attrs: &[
                        (
                            "d",
                            AttrVal::Lit(
                                "M55.2 13.7a30 30 0 0 1-41.5 41.5 33.5 33.5 0 0 0 41.5-41.5",
                            ),
                        ),
                        ("fill", AttrVal::Lit("#000")),
                        ("fill-opacity", AttrVal::Lit(".13")),
                    ],
                    children: &[],
                },
            ],
        },
    ]),
};

static COMP_SURFACE: ComponentDef = ComponentDef {
    name: "surface",
    width: Some(68.0),
    height: Some(66.0),
    probability: Some(95.0),
    translate: None,
    rotate: Some(Range(-180.0, 180.0)),

    variants: Variants::new(&[
        VariantDef {
            name: "banded",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-banded"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-banded)"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".16")),
                                    ("d", AttrVal::Lit("M2 9h64v5.5H2z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".13")),
                                    ("d", AttrVal::Lit("M2 18h64v6.5H2z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".14")),
                                    ("d", AttrVal::Lit("M2 29h64v8H2z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".15")),
                                    ("d", AttrVal::Lit("M2 41h64v6H2z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                    ("d", AttrVal::Lit("M2 51h64v5H2z")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "belted",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-belted"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-belted)"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".19")),
                                    ("d", AttrVal::Lit("M2 23h64v8.5H2z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".14")),
                                    ("d", AttrVal::Lit("M2 35h64v6H2z")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "cap",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-cap"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-cap)"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".24")),
                                    ("d", AttrVal::Lit("M2 3h64v12H2z")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".1")),
                                    ("d", AttrVal::Lit("M2 53h64v10H2z")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "cracked",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-cracked"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-cracked)"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("m2 31 16-4 14 8 16-6 18 6")),
                                    ("stroke", AttrVal::Lit("#fff")),
                                    ("stroke-opacity", AttrVal::Lit(".3")),
                                    ("stroke-width", AttrVal::Lit(".8")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("m28 3 4 16-8 14 6 16-4 14")),
                                    ("stroke", AttrVal::Lit("#fff")),
                                    ("stroke-opacity", AttrVal::Lit(".3")),
                                    ("stroke-width", AttrVal::Lit(".8")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("m44 45 10 6 10-4")),
                                    ("stroke", AttrVal::Lit("#fff")),
                                    ("stroke-opacity", AttrVal::Lit(".25")),
                                    ("stroke-width", AttrVal::Lit(".8")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("32")),
                                    ("cy", AttrVal::Lit("35")),
                                    ("r", AttrVal::Lit("1.4")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".2")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("30")),
                                    ("cy", AttrVal::Lit("49")),
                                    ("r", AttrVal::Lit("1.1")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".16")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "cratered",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-cratered"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-cratered)"))],
                        children: &[
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("24")),
                                    ("cy", AttrVal::Lit("21")),
                                    ("r", AttrVal::Lit("5")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".13")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("44")),
                                    ("cy", AttrVal::Lit("28")),
                                    ("r", AttrVal::Lit("4")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("31")),
                                    ("cy", AttrVal::Lit("44")),
                                    ("r", AttrVal::Lit("6")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".13")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("18")),
                                    ("cy", AttrVal::Lit("35")),
                                    ("r", AttrVal::Lit("3")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("42")),
                                    ("cy", AttrVal::Lit("47")),
                                    ("r", AttrVal::Lit("3.5")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".11")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("36")),
                                    ("cy", AttrVal::Lit("12")),
                                    ("r", AttrVal::Lit("2.5")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("28")),
                                    ("cy", AttrVal::Lit("30")),
                                    ("r", AttrVal::Lit("2")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".1")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("47")),
                                    ("cy", AttrVal::Lit("38")),
                                    ("r", AttrVal::Lit("1.8")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".1")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "marbled",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-marbled"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-marbled)"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    (
                                        "d",
                                        AttrVal::Lit(
                                            "M14 21c4-8 16-10 22-5s12 3 14 11-8 10-16 8-10 \
                                             0-16-4-8-2-4-10",
                                        ),
                                    ),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".11")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    (
                                        "d",
                                        AttrVal::Lit("M28 43c8-6 20-4 24 2s-4 12-14 12-14-4-10-14"),
                                    ),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".13")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("M12 39c4-4 10-2 10 3s-8 5-11 2-1-3 1-5")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".09")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    ("d", AttrVal::Lit("M42 9c6-2 12 2 12 7s-8 5-13 2-3-7 1-9")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".11")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "speckled",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-speckled"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-speckled)"))],
                        children: &[
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("15.4")),
                                    ("cy", AttrVal::Lit("38.7")),
                                    ("r", AttrVal::Lit("1.8")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".11")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("24.5")),
                                    ("cy", AttrVal::Lit("27.2")),
                                    ("r", AttrVal::Lit("1.3")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".1")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("18.1")),
                                    ("cy", AttrVal::Lit("16.1")),
                                    ("r", AttrVal::Lit(".8")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".09")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("31.4")),
                                    ("cy", AttrVal::Lit("40.4")),
                                    ("r", AttrVal::Lit("1.7")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".13")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("32.1")),
                                    ("cy", AttrVal::Lit("28")),
                                    ("r", AttrVal::Lit("1.9")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".14")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("54.5")),
                                    ("cy", AttrVal::Lit("28.4")),
                                    ("r", AttrVal::Lit("1.4")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".14")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("35.7")),
                                    ("cy", AttrVal::Lit("35.7")),
                                    ("r", AttrVal::Lit("1.3")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".13")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("44.6")),
                                    ("cy", AttrVal::Lit("37.1")),
                                    ("r", AttrVal::Lit("1")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("51.4")),
                                    ("cy", AttrVal::Lit("36.3")),
                                    ("r", AttrVal::Lit("1.2")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("44.3")),
                                    ("cy", AttrVal::Lit("17.3")),
                                    ("r", AttrVal::Lit("1.5")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".15")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("12.8")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("1.2")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("29.4")),
                                    ("cy", AttrVal::Lit("58.6")),
                                    ("r", AttrVal::Lit("1.9")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".11")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("29")),
                                    ("cy", AttrVal::Lit("44.4")),
                                    ("r", AttrVal::Lit("1")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".16")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("54.6")),
                                    ("cy", AttrVal::Lit("42.7")),
                                    ("r", AttrVal::Lit("1.2")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("45.9")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("1.8")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".1")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("8.7")),
                                    ("cy", AttrVal::Lit("37.8")),
                                    ("r", AttrVal::Lit("1.2")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".1")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("36.4")),
                                    ("cy", AttrVal::Lit("19.7")),
                                    ("r", AttrVal::Lit(".8")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".13")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("21.3")),
                                    ("cy", AttrVal::Lit("55.2")),
                                    ("r", AttrVal::Lit("1.6")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".14")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "spotted",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-spotted"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-spotted)"))],
                        children: &[
                            Node::El {
                                name: "ellipse",
                                attrs: &[
                                    ("cx", AttrVal::Lit("44")),
                                    ("cy", AttrVal::Lit("27")),
                                    ("rx", AttrVal::Lit("11")),
                                    ("ry", AttrVal::Lit("7")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".08")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "ellipse",
                                attrs: &[
                                    ("cx", AttrVal::Lit("44")),
                                    ("cy", AttrVal::Lit("27")),
                                    ("rx", AttrVal::Lit("9")),
                                    ("ry", AttrVal::Lit("5.5")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".22")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "ellipse",
                                attrs: &[
                                    ("cx", AttrVal::Lit("44")),
                                    ("cy", AttrVal::Lit("27")),
                                    ("rx", AttrVal::Lit("5")),
                                    ("ry", AttrVal::Lit("3")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".18")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("20")),
                                    ("cy", AttrVal::Lit("41")),
                                    ("r", AttrVal::Lit("3")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".15")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("26")),
                                    ("cy", AttrVal::Lit("15")),
                                    ("r", AttrVal::Lit("2.2")),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".1")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("42")),
                                    ("cy", AttrVal::Lit("47")),
                                    ("r", AttrVal::Lit("2.5")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "swirl",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-swirl"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-swirl)"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    (
                                        "d",
                                        AttrVal::Lit(
                                            "M2 23c12-8 26 4 38 0s22-14 28-8v14c-10-4-20 8-32 \
                                             10s-22-6-34-2Z",
                                        ),
                                    ),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".16")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "path",
                                attrs: &[
                                    (
                                        "d",
                                        AttrVal::Lit(
                                            "M4 45c12-6 24 4 38 1s18-7 24-3v10c-10-2-20 6-32 \
                                             4s-18-4-30-2Z",
                                        ),
                                    ),
                                    ("fill", AttrVal::Lit("#000")),
                                    ("fill-opacity", AttrVal::Lit(".12")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("44")),
                                    ("cy", AttrVal::Lit("19")),
                                    ("r", AttrVal::Lit("4.5")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".18")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
        VariantDef {
            name: "terra",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-surface"))],
                children: &[
                    Node::El {
                        name: "defs",
                        attrs: &[],
                        children: &[Node::El {
                            name: "clipPath",
                            attrs: &[("id", AttrVal::Lit("dicebearPlanets-terra"))],
                            children: &[Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("33")),
                                    ("r", AttrVal::Lit("30")),
                                ],
                                children: &[],
                            }],
                        }],
                    },
                    Node::El {
                        name: "g",
                        attrs: &[("clip-path", AttrVal::Lit("url(#dicebearPlanets-terra)"))],
                        children: &[
                            Node::El {
                                name: "path",
                                attrs: &[
                                    (
                                        "d",
                                        AttrVal::Lit(
                                            "M40.7 27c0 1.3-.2 2.6-.5 3.8-.4 1.2-.8 2.6-1.6 \
                                             3.5s-2.1 1.6-3.3 \
                                             2c-1.2.3-2.8.1-3.9.1s-2-.3-2.9-.1-1.6.9-2.5 \
                                             1.3c-.9.3-2.1 1-3.1 \
                                             1-1-.1-2.1-.7-2.7-1.5-.7-.7-.9-2.1-1.3-3-.3-1-.4-1.\
                                             8-.9-2.5-.6-.7-1.6-1-2.5-1.8s-2.3-1.7-2.9-2.8c-.7-1.\
                                             1-1.1-2.6-1-3.9.1-1.2.8-2.5 1.4-3.6s1.5-2.2 \
                                             2.5-3c.9-.9 2-1.8 3.2-2.1s2.8-.3 4 .3c1.2.5 2.5 2 \
                                             3.3 3s1.1 2.5 1.7 3.1c.5.5.8.5 1.7.3.9-.1 2.3-1 \
                                             3.7-1.2 1.3-.1 3.2-.1 4.4.4 1.2.6 2.2 1.8 2.7 2.9s.5 \
                                             2.5.5 3.8M56 44c-.1.7-.5 1.4-.9 1.9s-1.1.9-1.7 \
                                             1.2c-.5.4-1 .6-1.4.9s-.7.7-1.1 1.1c-.3.5-.6 1.1-1.1 \
                                             1.5-.5.5-1.1 1.1-1.8 1.4-.7.2-1.5.4-2.3.4-.7 \
                                             0-1.5-.2-2.3-.5-.7-.3-1.4-.7-2-1.3-.6-.5-1.2-1.2-1.\
                                             4-2-.3-.7-.4-1.6-.2-2.4s.7-1.6 1.3-2.2c.5-.6 1.3-1 \
                                             1.9-1.3.5-.4 1-.5 1.3-.9.2-.3.1-.7.2-1.3 0-.7-.2-1.7 \
                                             0-2.5.2-.9.6-2.1 1.2-2.7.5-.7 1.5-1.2 2.3-1.2.8-.1 \
                                             1.7.4 2.4.9s1.2 1.4 1.6 2c.5.7.7 1.4 1.1 1.9.4.6.8.9 \
                                             1.2 1.5.4.5 1 1 1.3 1.6s.5 1.3.4 2",
                                        ),
                                    ),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".26")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("48")),
                                    ("cy", AttrVal::Lit("19")),
                                    ("r", AttrVal::Lit("1.7")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".26")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("53")),
                                    ("cy", AttrVal::Lit("24")),
                                    ("r", AttrVal::Lit("1.2")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".26")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("43")),
                                    ("cy", AttrVal::Lit("15")),
                                    ("r", AttrVal::Lit("1.1")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".26")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "circle",
                                attrs: &[
                                    ("cx", AttrVal::Lit("19")),
                                    ("cy", AttrVal::Lit("49")),
                                    ("r", AttrVal::Lit("1.4")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".26")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "ellipse",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("5.5")),
                                    ("rx", AttrVal::Lit("15")),
                                    ("ry", AttrVal::Lit("5.5")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".42")),
                                ],
                                children: &[],
                            },
                            Node::El {
                                name: "ellipse",
                                attrs: &[
                                    ("cx", AttrVal::Lit("34")),
                                    ("cy", AttrVal::Lit("61")),
                                    ("rx", AttrVal::Lit("11")),
                                    ("ry", AttrVal::Lit("4.5")),
                                    ("fill", AttrVal::Lit("#fff")),
                                    ("fill-opacity", AttrVal::Lit(".38")),
                                ],
                                children: &[],
                            },
                        ],
                    },
                ],
            }],
        },
    ]),
};

static COMP_STAR: ComponentDef = ComponentDef {
    name: "star",
    width: Some(10.0),
    height: Some(10.0),
    probability: Some(90.0),
    translate: Some((Range(-120.0, 120.0), Range(-120.0, 120.0))),
    rotate: None,

    variants: Variants::new(&[
        VariantDef {
            name: "faint",
            weight: 2.0,
            tags: &[],
            elements: &[Node::El {
                name: "circle",
                attrs: &[
                    ("cx", AttrVal::Lit("5")),
                    ("cy", AttrVal::Lit("5")),
                    ("r", AttrVal::Lit(".7")),
                    ("fill", AttrVal::Lit("#fff")),
                    ("fill-opacity", AttrVal::Lit(".5")),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "large",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-tw-large"))],
                children: &[Node::El {
                    name: "circle",
                    attrs: &[
                        ("cx", AttrVal::Lit("5")),
                        ("cy", AttrVal::Lit("5")),
                        ("r", AttrVal::Lit("1.4")),
                        ("fill", AttrVal::Lit("#fff")),
                        ("fill-opacity", AttrVal::Lit(".9")),
                    ],
                    children: &[],
                }],
            }],
        },
        VariantDef {
            name: "medium",
            weight: 3.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-tw-medium"))],
                children: &[Node::El {
                    name: "circle",
                    attrs: &[
                        ("cx", AttrVal::Lit("5")),
                        ("cy", AttrVal::Lit("5")),
                        ("r", AttrVal::Lit("1.1")),
                        ("fill", AttrVal::Lit("#fff")),
                        ("fill-opacity", AttrVal::Lit(".85")),
                    ],
                    children: &[],
                }],
            }],
        },
        VariantDef {
            name: "small",
            weight: 3.0,
            tags: &[],
            elements: &[Node::El {
                name: "circle",
                attrs: &[
                    ("cx", AttrVal::Lit("5")),
                    ("cy", AttrVal::Lit("5")),
                    ("r", AttrVal::Lit(".8")),
                    ("fill", AttrVal::Lit("#fff")),
                    ("fill-opacity", AttrVal::Lit(".85")),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "sparkle",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "g",
                attrs: &[("class", AttrVal::Lit("dbpa-tw-sparkle"))],
                children: &[Node::El {
                    name: "path",
                    attrs: &[
                        (
                            "d",
                            AttrVal::Lit(
                                "M5 1.8Q5.5 4.5 8.2 5 5.5 5.5 5 8.2 4.5 5.5 1.8 5 4.5 4.5 5 1.8",
                            ),
                        ),
                        ("fill", AttrVal::Lit("#fff")),
                        ("fill-opacity", AttrVal::Lit(".9")),
                    ],
                    children: &[],
                }],
            }],
        },
    ]),
};

static COMP_ANIMATION: ComponentDef = ComponentDef {
    name: "animation",
    width: Some(100.0),
    height: Some(100.0),
    probability: None,
    translate: None,
    rotate: None,

    variants: Variants::new(&[
        VariantDef {
            name: "fast",
            weight: 0.0,
            tags: &["animation"],
            elements: &[
                Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbpa-fast"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbpa-fast){--dbpa-t:0.9;--dbpa-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbpaSpin{to{transform:rotate(360deg)}} @keyframes \
                                dbpaTw{0%,100%{opacity:1}50%{opacity:.3}} \
                                .dbpa-surface{transform-origin:34px 33px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*100s) linear infinite} \
                                .dbpa-moons{transform-origin:29px 29px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*42s) linear infinite} \
                                .dbpa-tw-medium{animation:dbpaTw calc(var(--dbpa-t,1)*5.2s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2s)} \
                                .dbpa-tw-large{animation:dbpaTw calc(var(--dbpa-t,1)*3.7s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2.7s)} \
                                .dbpa-tw-sparkle{animation:dbpaTw calc(var(--dbpa-t,1)*2.8s) \
                                ease-in-out infinite \
                                calc(var(--dbpa-t,1)*1.4s)}.dbpa-surface,.dbpa-moons,.\
                                dbpa-tw-medium,.dbpa-tw-large,.\
                                dbpa-tw-sparkle{animation-play-state:var(--dbpa-p,paused)}}",
                    }],
                },
            ],
        },
        VariantDef {
            name: "fastest",
            weight: 0.0,
            tags: &["animation"],
            elements: &[
                Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbpa-fastest"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbpa-fastest){--dbpa-t:0.75;--dbpa-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbpaSpin{to{transform:rotate(360deg)}} @keyframes \
                                dbpaTw{0%,100%{opacity:1}50%{opacity:.3}} \
                                .dbpa-surface{transform-origin:34px 33px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*100s) linear infinite} \
                                .dbpa-moons{transform-origin:29px 29px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*42s) linear infinite} \
                                .dbpa-tw-medium{animation:dbpaTw calc(var(--dbpa-t,1)*5.2s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2s)} \
                                .dbpa-tw-large{animation:dbpaTw calc(var(--dbpa-t,1)*3.7s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2.7s)} \
                                .dbpa-tw-sparkle{animation:dbpaTw calc(var(--dbpa-t,1)*2.8s) \
                                ease-in-out infinite \
                                calc(var(--dbpa-t,1)*1.4s)}.dbpa-surface,.dbpa-moons,.\
                                dbpa-tw-medium,.dbpa-tw-large,.\
                                dbpa-tw-sparkle{animation-play-state:var(--dbpa-p,paused)}}",
                    }],
                },
            ],
        },
        VariantDef {
            name: "medium",
            weight: 0.0,
            tags: &["animation"],
            elements: &[
                Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbpa-medium"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbpa-medium){--dbpa-t:1;--dbpa-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbpaSpin{to{transform:rotate(360deg)}} @keyframes \
                                dbpaTw{0%,100%{opacity:1}50%{opacity:.3}} \
                                .dbpa-surface{transform-origin:34px 33px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*100s) linear infinite} \
                                .dbpa-moons{transform-origin:29px 29px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*42s) linear infinite} \
                                .dbpa-tw-medium{animation:dbpaTw calc(var(--dbpa-t,1)*5.2s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2s)} \
                                .dbpa-tw-large{animation:dbpaTw calc(var(--dbpa-t,1)*3.7s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2.7s)} \
                                .dbpa-tw-sparkle{animation:dbpaTw calc(var(--dbpa-t,1)*2.8s) \
                                ease-in-out infinite \
                                calc(var(--dbpa-t,1)*1.4s)}.dbpa-surface,.dbpa-moons,.\
                                dbpa-tw-medium,.dbpa-tw-large,.\
                                dbpa-tw-sparkle{animation-play-state:var(--dbpa-p,paused)}}",
                    }],
                },
            ],
        },
        VariantDef {
            name: "none",
            weight: 1.0,
            tags: &[],
            elements: &[],
        },
        VariantDef {
            name: "slow",
            weight: 0.0,
            tags: &["animation"],
            elements: &[
                Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbpa-slow"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbpa-slow){--dbpa-t:1.15;--dbpa-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbpaSpin{to{transform:rotate(360deg)}} @keyframes \
                                dbpaTw{0%,100%{opacity:1}50%{opacity:.3}} \
                                .dbpa-surface{transform-origin:34px 33px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*100s) linear infinite} \
                                .dbpa-moons{transform-origin:29px 29px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*42s) linear infinite} \
                                .dbpa-tw-medium{animation:dbpaTw calc(var(--dbpa-t,1)*5.2s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2s)} \
                                .dbpa-tw-large{animation:dbpaTw calc(var(--dbpa-t,1)*3.7s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2.7s)} \
                                .dbpa-tw-sparkle{animation:dbpaTw calc(var(--dbpa-t,1)*2.8s) \
                                ease-in-out infinite \
                                calc(var(--dbpa-t,1)*1.4s)}.dbpa-surface,.dbpa-moons,.\
                                dbpa-tw-medium,.dbpa-tw-large,.\
                                dbpa-tw-sparkle{animation-play-state:var(--dbpa-p,paused)}}",
                    }],
                },
            ],
        },
        VariantDef {
            name: "slowest",
            weight: 0.0,
            tags: &["animation"],
            elements: &[
                Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbpa-slowest"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbpa-slowest){--dbpa-t:1.35;--dbpa-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbpaSpin{to{transform:rotate(360deg)}} @keyframes \
                                dbpaTw{0%,100%{opacity:1}50%{opacity:.3}} \
                                .dbpa-surface{transform-origin:34px 33px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*100s) linear infinite} \
                                .dbpa-moons{transform-origin:29px 29px;animation:dbpaSpin \
                                calc(var(--dbpa-t,1)*42s) linear infinite} \
                                .dbpa-tw-medium{animation:dbpaTw calc(var(--dbpa-t,1)*5.2s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2s)} \
                                .dbpa-tw-large{animation:dbpaTw calc(var(--dbpa-t,1)*3.7s) \
                                ease-in-out infinite calc(var(--dbpa-t,1)*2.7s)} \
                                .dbpa-tw-sparkle{animation:dbpaTw calc(var(--dbpa-t,1)*2.8s) \
                                ease-in-out infinite \
                                calc(var(--dbpa-t,1)*1.4s)}.dbpa-surface,.dbpa-moons,.\
                                dbpa-tw-medium,.dbpa-tw-large,.\
                                dbpa-tw-sparkle{animation-play-state:var(--dbpa-p,paused)}}",
                    }],
                },
            ],
        },
    ]),
};

static CANVAS: &[Node] = &[
    Node::Component {
        name: "star",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(7.5 7.5)"))],
    },
    Node::Component {
        name: "star02",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(32.5 7.5)"))],
    },
    Node::Component {
        name: "star03",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(57.5 7.5)"))],
    },
    Node::Component {
        name: "star04",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(82.5 7.5)"))],
    },
    Node::Component {
        name: "star05",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(7.5 32.5)"))],
    },
    Node::Component {
        name: "star06",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(82.5 32.5)"))],
    },
    Node::Component {
        name: "star07",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(7.5 57.5)"))],
    },
    Node::Component {
        name: "star08",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(82.5 57.5)"))],
    },
    Node::Component {
        name: "star09",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(7.5 82.5)"))],
    },
    Node::Component {
        name: "star10",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(32.5 82.5)"))],
    },
    Node::Component {
        name: "star11",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(57.5 82.5)"))],
    },
    Node::Component {
        name: "star12",
        component: &COMP_STAR,
        attrs: &[("transform", AttrVal::Lit("translate(82.5 82.5)"))],
    },
    Node::Component {
        name: "planet",
        component: &COMP_PLANET,
        attrs: &[("transform", AttrVal::Lit("translate(20 20)"))],
    },
    Node::Component {
        name: "surface",
        component: &COMP_SURFACE,
        attrs: &[("transform", AttrVal::Lit("translate(16 17)"))],
    },
    Node::Component {
        name: "shade",
        component: &COMP_SHADE,
        attrs: &[("transform", AttrVal::Lit("translate(20 20)"))],
    },
    Node::Component {
        name: "ring",
        component: &COMP_RING,
        attrs: &[("transform", AttrVal::Lit("translate(2 34)"))],
    },
    Node::Component {
        name: "moons",
        component: &COMP_MOONS,
        attrs: &[("transform", AttrVal::Lit("translate(21 21)"))],
    },
    Node::Component {
        name: "animation",
        component: &COMP_ANIMATION,
        attrs: &[],
    },
];

const BG: ColorRef = ColorRef {
    contrast_to: None,
    not_equal_to: &[],
    key: "background",
    palette: Palette::new(&[
        "#002a2e", "#012e3a", "#0b3533", "#0f2336", "#17233f", "#1c1f27", "#1d1a2a", "#23244a",
        "#2c1c45", "#361b34",
    ]),
};
const PLANET: ColorRef = ColorRef {
    contrast_to: None,
    not_equal_to: &[],
    key: "planet",
    palette: Palette::new(&[
        "#00b1cf", "#00b6af", "#39b789", "#47a7e7", "#74b160", "#7a9bef", "#9fa63b", "#a18ee8",
        "#c083d2", "#c1982a", "#d67cb2", "#d88a40", "#e27a8c", "#e37f64",
    ]),
};
const MOON: ColorRef = ColorRef {
    contrast_to: None,
    not_equal_to: &[],
    key: "moon",
    palette: Palette::new(&["#bed9e7", "#d3d7e2", "#e0ded8", "#e5d6b6", "#eaccc7"]),
};

// Curly quotes in METADATA are required for byte parity with DiceBear.
const METADATA: &str = r#"<metadata xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><rdf:RDF><rdf:Description><dc:title>Planets</dc:title><dc:creator>DiceBear</dc:creator><dc:source xsi:type="dcterms:URI">https://www.dicebear.com</dc:source><dcterms:license xsi:type="dcterms:URI">https://creativecommons.org/publicdomain/zero/1.0/</dcterms:license><dc:rights>“Planets” (https://www.dicebear.com) by “DiceBear”, licensed under “CC0 1.0” (https://creativecommons.org/publicdomain/zero/1.0/)</dc:rights></rdf:Description></rdf:RDF></metadata>"#;

const COLORS: &[ColorRef] = &[BG, PLANET, MOON];

pub static PLANETS: Style = Style {
    source_name: "Planets",
    metadata: METADATA,
    canvas_w: 100.0,
    canvas_h: 100.0,
    canvas: Canvas::new(CANVAS),
    colors: COLORS,
};
