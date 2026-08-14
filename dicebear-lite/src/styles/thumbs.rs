// DiceBear 10.x thumbs style (CC0 1.0).

use crate::Style;
use crate::color::Rgb8;
use crate::data::{
    AttrVal, Canvas, ColorRef, ComponentDef, Node, Palette, Range, VariantDef, Variants,
};

static COMP_BODY: ComponentDef = ComponentDef {
    name: "body",
    width: Some(90.0),
    height: Some(130.0),
    probability: None,
    translate: Some((Range(-5.0, 5.0), Range(-5.0, 5.0))),
    rotate: Some(Range(-20.0, 20.0)),

    variants: Variants::new(&[VariantDef {
        name: "default",
        weight: 1.0,
        tags: &[],
        elements: &[
            Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M45 0c24.85 0 45 19.4 45 43.33V130H0V43.33C0 19.4 20.15 0 45 0",
                        ),
                    ),
                    ("fill", AttrVal::Color(SHAPE)),
                ],
                children: &[],
            },
            Node::Component {
                name: "head",
                component: &COMP_HEAD,
                attrs: &[("transform", AttrVal::Lit("translate(24 24)"))],
            },
        ],
    }]),
};

static COMP_EYES: ComponentDef = ComponentDef {
    name: "eyes",
    width: Some(16.0),
    height: Some(16.0),
    probability: None,
    translate: Some((Range(-20.0, 20.0), Range(-10.0, 10.0))),
    rotate: None,

    variants: Variants::new(&[
        VariantDef {
            name: "variant01",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M.25 8.12C1.66 11.86 12 16 12 16s5.17-9.58 3.76-13.32c0 \
                             0-1.41-3.74-5.3-2.38-3.87 1.36-2.7 4.48-2.7 4.48S6.6 1.66 2.73 \
                             3.02.25 8.12.25 8.12",
                        ),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant02",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M9.5 10c-3.88 0-7.11-4.23-6.4-4.85s2.63 1.3 6.4 1.3 5.69-2 \
                             6.4-1.3S13.38 10 9.5 10",
                        ),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant03",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M11.86 7.5c0-1.42-4.14-2.85-4.82-4.98C6.34.4 16 5.37 16 7.5s-9.65 \
                             7.11-8.96 4.98 4.82-3.56 4.82-4.98",
                        ),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant04",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M8 8.36S8 4 12 4s4 4.36 4 4.36v2.91s0 .73-.67.73c-.66 \
                             0-.66-2.9-3.33-2.9S9.33 12 8.67 12 8 11.27 8 11.27z",
                        ),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant05",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit("M16 8c0 2.2-1.34 4-3 4s-3-1.8-3-4 1.34-4 3-4 3 1.8 3 4"),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant06",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit("M16 8c0 3.31-1.34 6-3 6s-3-2.69-3-6 1.34-6 3-6 3 2.69 3 6"),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant07",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M11.5 6C8.29 6 7 7.36 7 8.04c0 3.4 1.29 1.35 4.5 1.35S16 11.43 16 \
                             8.04C16 7.36 14.71 6 11.5 6",
                        ),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant08",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M16 8c0 1.66-1.12 3-2.5 3S11 9.66 11 8s1.12-3 2.5-3S16 6.34 16 8",
                        ),
                    ),
                    ("fill", AttrVal::Color(EYES)),
                ],
                children: &[],
            }],
        },
    ]),
};

static COMP_HEAD: ComponentDef = ComponentDef {
    name: "head",
    width: Some(44.0),
    height: Some(40.0),
    probability: None,
    translate: Some((Range(-15.0, 15.0), Range(-15.0, 15.0))),
    rotate: Some(Range(-20.0, 20.0)),

    variants: Variants::new(&[VariantDef {
        name: "default",
        weight: 1.0,
        tags: &[],
        elements: &[Node::El {
            name: "g",
            attrs: &[("class", AttrVal::Lit("dbth-f"))],
            children: &[
                Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbth-eyes"))],
                    children: &[Node::Component {
                        name: "eyes",
                        component: &COMP_EYES,
                        attrs: &[],
                    }],
                },
                Node::El {
                    name: "g",
                    attrs: &[("class", AttrVal::Lit("dbth-eyes"))],
                    children: &[Node::Component {
                        name: "eyes",
                        component: &COMP_EYES,
                        attrs: &[("transform", AttrVal::Lit("matrix(-1 0 0 1 44 0)"))],
                    }],
                },
                Node::Component {
                    name: "mouth",
                    component: &COMP_MOUTH,
                    attrs: &[("transform", AttrVal::Lit("translate(7 26)"))],
                },
            ],
        }],
    }]),
};

static COMP_MOUTH: ComponentDef = ComponentDef {
    name: "mouth",
    width: Some(30.0),
    height: Some(14.0),
    probability: None,
    translate: Some((Range(0.0, 0.0), Range(-10.0, 10.0))),
    rotate: None,

    variants: Variants::new(&[
        VariantDef {
            name: "variant01",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M15 11C4.52 11 2.42 2.82 3.12 2.14S8.02 3.5 15 3.5s11.18-2.04 \
                             11.88-1.36S25.48 11 15 11",
                        ),
                    ),
                    ("fill", AttrVal::Color(MOUTH)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant02",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M15 14C1.9 14-.72 1.29.15.23S6.27 2.11 15 2.11 28.97-.83 29.85.23 \
                             28.1 14 15 14",
                        ),
                    ),
                    ("fill", AttrVal::Color(MOUTH)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant03",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M15.5 10c-5.07 0-9.3-5.23-8.37-5.88s3.45 2.15 8.37 2.15 7.44-2.88 \
                             8.37-2.15S20.57 10 15.5 10",
                        ),
                    ),
                    ("fill", AttrVal::Color(MOUTH)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant04",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M15 10C6.79 10 3.02 3.88 4.22 3.12 5.42 2.35 6.1 6.6 15 6.49c8.9-.12 \
                             9.58-4.23 10.78-3.37S23.21 10 15 10",
                        ),
                    ),
                    ("fill", AttrVal::Color(MOUTH)),
                ],
                children: &[],
            }],
        },
        VariantDef {
            name: "variant05",
            weight: 1.0,
            tags: &[],
            elements: &[Node::El {
                name: "path",
                attrs: &[
                    (
                        "d",
                        AttrVal::Lit(
                            "M15.2 3.84c0-.67-4.2-2-4.2-2.67s7 .67 7 2.67-4.2 2.66-4.2 2.66 \
                             4.2.67 4.2 2.66-7 3.33-7 2.67 4.2-2 4.2-2.67-3.5-1.33-3.5-2.66 3.5-2 \
                             3.5-2.66",
                        ),
                    ),
                    ("fill", AttrVal::Color(MOUTH)),
                ],
                children: &[],
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
                    attrs: &[("class", AttrVal::Lit("dbth-fast"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbth-fast){--dbth-t:0.9;--dbth-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbthBlink{0%,91%,97%,100%{transform:scaleY(1)}94%{transform:\
                                scaleY(0.12)}}@keyframes \
                                dbthSway{0%{transform:rotate(0deg);animation-timing-function:\
                                ease-out}25%{transform:rotate(1.2deg);animation-timing-function:\
                                ease-in-out}75%{transform:rotate(-1.2deg);\
                                animation-timing-function:ease-in}100%{transform:rotate(0deg)}}@\
                                keyframes dbthFace{0%{transform:translateX(0) \
                                rotate(0deg);animation-timing-function:ease-out}30%{transform:\
                                translateX(1px) \
                                rotate(1.5deg);animation-timing-function:ease-in-out}80%\
                                {transform:translateX(-1px) \
                                rotate(-1.5deg);animation-timing-function:ease-in}100%{transform:\
                                translateX(0) rotate(0deg)}} \
                                .dbth-eyes{transform-box:fill-box;transform-origin:center;\
                                animation:dbthBlink calc(var(--dbth-t,1)*4.6s) linear infinite} \
                                .dbth-c{transform-box:fill-box;transform-origin:50% \
                                100%;animation:dbthSway calc(var(--dbth-t,1)*5.2s) linear \
                                infinite} .dbth-f{transform-box:fill-box;transform-origin:center;\
                                animation:dbthFace calc(var(--dbth-t,1)*5.2s) linear \
                                infinite}.dbth-c,.dbth-eyes,.dbth-f{animation-play-state:\
                                var(--dbth-p,paused)}}",
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
                    attrs: &[("class", AttrVal::Lit("dbth-fastest"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbth-fastest){--dbth-t:0.75;--dbth-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbthBlink{0%,91%,97%,100%{transform:scaleY(1)}94%{transform:\
                                scaleY(0.12)}}@keyframes \
                                dbthSway{0%{transform:rotate(0deg);animation-timing-function:\
                                ease-out}25%{transform:rotate(1.2deg);animation-timing-function:\
                                ease-in-out}75%{transform:rotate(-1.2deg);\
                                animation-timing-function:ease-in}100%{transform:rotate(0deg)}}@\
                                keyframes dbthFace{0%{transform:translateX(0) \
                                rotate(0deg);animation-timing-function:ease-out}30%{transform:\
                                translateX(1px) \
                                rotate(1.5deg);animation-timing-function:ease-in-out}80%\
                                {transform:translateX(-1px) \
                                rotate(-1.5deg);animation-timing-function:ease-in}100%{transform:\
                                translateX(0) rotate(0deg)}} \
                                .dbth-eyes{transform-box:fill-box;transform-origin:center;\
                                animation:dbthBlink calc(var(--dbth-t,1)*4.6s) linear infinite} \
                                .dbth-c{transform-box:fill-box;transform-origin:50% \
                                100%;animation:dbthSway calc(var(--dbth-t,1)*5.2s) linear \
                                infinite} .dbth-f{transform-box:fill-box;transform-origin:center;\
                                animation:dbthFace calc(var(--dbth-t,1)*5.2s) linear \
                                infinite}.dbth-c,.dbth-eyes,.dbth-f{animation-play-state:\
                                var(--dbth-p,paused)}}",
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
                    attrs: &[("class", AttrVal::Lit("dbth-medium"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbth-medium){--dbth-t:1;--dbth-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbthBlink{0%,91%,97%,100%{transform:scaleY(1)}94%{transform:\
                                scaleY(0.12)}}@keyframes \
                                dbthSway{0%{transform:rotate(0deg);animation-timing-function:\
                                ease-out}25%{transform:rotate(1.2deg);animation-timing-function:\
                                ease-in-out}75%{transform:rotate(-1.2deg);\
                                animation-timing-function:ease-in}100%{transform:rotate(0deg)}}@\
                                keyframes dbthFace{0%{transform:translateX(0) \
                                rotate(0deg);animation-timing-function:ease-out}30%{transform:\
                                translateX(1px) \
                                rotate(1.5deg);animation-timing-function:ease-in-out}80%\
                                {transform:translateX(-1px) \
                                rotate(-1.5deg);animation-timing-function:ease-in}100%{transform:\
                                translateX(0) rotate(0deg)}} \
                                .dbth-eyes{transform-box:fill-box;transform-origin:center;\
                                animation:dbthBlink calc(var(--dbth-t,1)*4.6s) linear infinite} \
                                .dbth-c{transform-box:fill-box;transform-origin:50% \
                                100%;animation:dbthSway calc(var(--dbth-t,1)*5.2s) linear \
                                infinite} .dbth-f{transform-box:fill-box;transform-origin:center;\
                                animation:dbthFace calc(var(--dbth-t,1)*5.2s) linear \
                                infinite}.dbth-c,.dbth-eyes,.dbth-f{animation-play-state:\
                                var(--dbth-p,paused)}}",
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
                    attrs: &[("class", AttrVal::Lit("dbth-slow"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbth-slow){--dbth-t:1.15;--dbth-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbthBlink{0%,91%,97%,100%{transform:scaleY(1)}94%{transform:\
                                scaleY(0.12)}}@keyframes \
                                dbthSway{0%{transform:rotate(0deg);animation-timing-function:\
                                ease-out}25%{transform:rotate(1.2deg);animation-timing-function:\
                                ease-in-out}75%{transform:rotate(-1.2deg);\
                                animation-timing-function:ease-in}100%{transform:rotate(0deg)}}@\
                                keyframes dbthFace{0%{transform:translateX(0) \
                                rotate(0deg);animation-timing-function:ease-out}30%{transform:\
                                translateX(1px) \
                                rotate(1.5deg);animation-timing-function:ease-in-out}80%\
                                {transform:translateX(-1px) \
                                rotate(-1.5deg);animation-timing-function:ease-in}100%{transform:\
                                translateX(0) rotate(0deg)}} \
                                .dbth-eyes{transform-box:fill-box;transform-origin:center;\
                                animation:dbthBlink calc(var(--dbth-t,1)*4.6s) linear infinite} \
                                .dbth-c{transform-box:fill-box;transform-origin:50% \
                                100%;animation:dbthSway calc(var(--dbth-t,1)*5.2s) linear \
                                infinite} .dbth-f{transform-box:fill-box;transform-origin:center;\
                                animation:dbthFace calc(var(--dbth-t,1)*5.2s) linear \
                                infinite}.dbth-c,.dbth-eyes,.dbth-f{animation-play-state:\
                                var(--dbth-p,paused)}}",
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
                    attrs: &[("class", AttrVal::Lit("dbth-slowest"))],
                    children: &[],
                },
                Node::El {
                    name: "style",
                    attrs: &[],
                    children: &[Node::Text {
                        value: "svg:has(.dbth-slowest){--dbth-t:1.35;--dbth-p:running}@media \
                                (prefers-reduced-motion: no-preference){@keyframes \
                                dbthBlink{0%,91%,97%,100%{transform:scaleY(1)}94%{transform:\
                                scaleY(0.12)}}@keyframes \
                                dbthSway{0%{transform:rotate(0deg);animation-timing-function:\
                                ease-out}25%{transform:rotate(1.2deg);animation-timing-function:\
                                ease-in-out}75%{transform:rotate(-1.2deg);\
                                animation-timing-function:ease-in}100%{transform:rotate(0deg)}}@\
                                keyframes dbthFace{0%{transform:translateX(0) \
                                rotate(0deg);animation-timing-function:ease-out}30%{transform:\
                                translateX(1px) \
                                rotate(1.5deg);animation-timing-function:ease-in-out}80%\
                                {transform:translateX(-1px) \
                                rotate(-1.5deg);animation-timing-function:ease-in}100%{transform:\
                                translateX(0) rotate(0deg)}} \
                                .dbth-eyes{transform-box:fill-box;transform-origin:center;\
                                animation:dbthBlink calc(var(--dbth-t,1)*4.6s) linear infinite} \
                                .dbth-c{transform-box:fill-box;transform-origin:50% \
                                100%;animation:dbthSway calc(var(--dbth-t,1)*5.2s) linear \
                                infinite} .dbth-f{transform-box:fill-box;transform-origin:center;\
                                animation:dbthFace calc(var(--dbth-t,1)*5.2s) linear \
                                infinite}.dbth-c,.dbth-eyes,.dbth-f{animation-play-state:\
                                var(--dbth-p,paused)}}",
                    }],
                },
            ],
        },
    ]),
};

static CANVAS: &[Node] = &[
    Node::El {
        name: "g",
        attrs: &[("class", AttrVal::Lit("dbth-c"))],
        children: &[Node::Component {
            name: "body",
            component: &COMP_BODY,
            attrs: &[("transform", AttrVal::Lit("translate(5 10)"))],
        }],
    },
    Node::Component {
        name: "animation",
        component: &COMP_ANIMATION,
        attrs: &[],
    },
];

#[expect(clippy::unreadable_literal, reason = "hex color values")]
const BG: ColorRef = ColorRef {
    key: "background",
    palette: Palette::new(&[
        Rgb8::from_u24(0x0A5B83),
        Rgb8::from_u24(0x1C799F),
        Rgb8::from_u24(0x69D2E7),
        Rgb8::from_u24(0xF1F4DC),
        Rgb8::from_u24(0xF88C49),
    ]),
    contrast_to: None,
    not_equal_to: &[],
};
#[expect(clippy::unreadable_literal, reason = "hex color values")]
const SHAPE: ColorRef = ColorRef {
    key: "shape",
    palette: Palette::new(&[
        Rgb8::from_u24(0x0A5B83),
        Rgb8::from_u24(0x1C799F),
        Rgb8::from_u24(0x69D2E7),
        Rgb8::from_u24(0xF1F4DC),
        Rgb8::from_u24(0xF88C49),
    ]),
    contrast_to: None,
    not_equal_to: &[&BG],
};
#[expect(clippy::unreadable_literal, reason = "hex color values")]
const EYES: ColorRef = ColorRef {
    key: "eyes",
    palette: Palette::new(&[Rgb8::from_u24(0x000000), Rgb8::from_u24(0xFFFFFF)]),
    contrast_to: Some(&SHAPE),
    not_equal_to: &[&SHAPE],
};
#[expect(clippy::unreadable_literal, reason = "hex color values")]
const MOUTH: ColorRef = ColorRef {
    key: "mouth",
    palette: Palette::new(&[Rgb8::from_u24(0x000000), Rgb8::from_u24(0xFFFFFF)]),
    contrast_to: Some(&SHAPE),
    not_equal_to: &[&SHAPE],
};

// Curly quotes in METADATA are required for byte parity with DiceBear.
const METADATA: &str = r#"<metadata xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><rdf:RDF><rdf:Description><dc:title>Thumbs</dc:title><dc:creator>DiceBear</dc:creator><dc:source xsi:type="dcterms:URI">https://www.dicebear.com</dc:source><dcterms:license xsi:type="dcterms:URI">https://creativecommons.org/publicdomain/zero/1.0/</dcterms:license><dc:rights>“Thumbs” (https://www.dicebear.com) by “DiceBear”, licensed under “CC0 1.0” (https://creativecommons.org/publicdomain/zero/1.0/)</dc:rights></rdf:Description></rdf:RDF></metadata>"#;

pub static THUMBS: Style = Style {
    source_name: "Thumbs",
    metadata: METADATA,
    canvas_w: 100.0,
    canvas_h: 100.0,
    canvas: Canvas::new(CANVAS),
    background: BG,
};
