//! SVG serializer. Walks the canvas element tree and streams markup
//! byte-for-byte compatible with `DiceBear`'s `Renderer` for default options,
//! without allocating.
//!
//! Output order is `<defs>` (component variant bodies deduped, then the
//! clipPath) followed by the clipped body (background plus `<use>` references).
//! Because `<defs>` must precede the body but is discovered while walking it,
//! the renderer makes two deterministic passes: variant selection is fully
//! keyed by `(seed, key)`, so recomputing it is safe.

use core::fmt::{self};

use crate::Style;
use crate::data::{AttrVal, ComponentDef, Node, VariantDef};
use crate::number::{Num, equals};
use crate::prng::hash_u32;
use crate::resolver::Resolver;
use crate::xml::Escaped;

const MAX_DEFS: usize = 16;

/// Dedup state for pass 1. Uses plain `&mut` — no interior mutability needed
/// because it lives as a local inside `Display::fmt`.
struct DefDedup<'a> {
    seen_g: [Option<(&'a str, &'a str)>; MAX_DEFS],
    seen_grad: [Option<&'a str>; MAX_DEFS],
}

impl<'a> DefDedup<'a> {
    const fn new() -> Self {
        Self {
            seen_g: [None; MAX_DEFS],
            seen_grad: [None; MAX_DEFS],
        }
    }

    fn seen_g(&self, source: &str, variant: &str) -> bool {
        self.seen_g
            .iter()
            .flatten()
            .any(|&(s, v)| s == source && v == variant)
    }

    fn mark_g(&mut self, source: &'a str, variant: &'a str) {
        for slot in &mut self.seen_g {
            if slot.is_none() {
                *slot = Some((source, variant));
                return;
            }
        }
    }

    fn seen_grad(&self, id: &str) -> bool {
        self.seen_grad.iter().flatten().any(|&s| s == id)
    }

    fn mark_grad(&mut self, id: &'a str) {
        for slot in &mut self.seen_grad {
            if slot.is_none() {
                *slot = Some(id);
                return;
            }
        }
    }
}

pub struct Renderer<'a> {
    style: &'a Style<'a>,
    seed: &'a str,
}

impl<'a> Renderer<'a> {
    pub const fn new(style: &'a Style<'a>, seed: &'a str) -> Self {
        Self { style, seed }
    }

    /// Writes a component's def: embedded `<defs>` (gradients) are diverted
    /// into the document `<defs>` (deduped by id), then the variant body is
    /// wrapped in `<g id="...">`.
    fn write_def<'b>(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        source: &str,
        variant: &VariantDef<'b>,
        hash: u32,
        dedup: &mut DefDedup<'b>,
    ) -> fmt::Result {
        self.divert_defs(f, resolver, variant.elements, dedup)?;

        write!(f, "<g id=\"{source}-{}-{hash:08x}\">", variant.name)?;
        for el in variant.elements {
            if !matches!(el, Node::El { name: "defs", .. }) {
                self.write_node(f, resolver, el)?;
            }
        }
        write!(f, "</g>")
    }

    /// Writes a `<use>` reference with the merged placement + component
    /// transform.
    #[expect(clippy::too_many_arguments, reason = "rendering parameters")]
    #[expect(clippy::unused_self, reason = "grouped with Renderer methods")]
    fn write_use(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        name: &str,
        source: &str,
        component: &ComponentDef<'_>,
        variant: &VariantDef<'_>,
        attrs: &[(&str, AttrVal<'_>)],
        hash: u32,
    ) -> fmt::Result {
        let transform = resolver.component_transform(name, component);
        let width = component.width.unwrap_or(0.0);
        let height = component.height.unwrap_or(0.0);
        let cx = width / 2.0;
        let cy = height / 2.0;

        let user_transform = attrs.iter().find_map(|(k, v)| match (*k, v) {
            ("transform", AttrVal::Lit(s)) => Some(*s),
            _ => None,
        });
        let has_translate =
            !equals(transform.translate_x, 0.0) || !equals(transform.translate_y, 0.0);
        let has_rotate = !equals(transform.rotate, 0.0);
        let has_scale = !equals(transform.scale, 1.0);

        write!(f, "<use")?;
        if user_transform.is_some() || has_translate || has_rotate || has_scale {
            write!(f, " transform=\"")?;
            let mut wrote = user_transform.is_some();
            if let Some(ut) = user_transform {
                write!(f, "{ut}")?;
            }
            if has_translate {
                if wrote {
                    write!(f, " ")?;
                }
                write!(
                    f,
                    "translate({}, {})",
                    Num(transform.translate_x / 100.0 * width),
                    Num(transform.translate_y / 100.0 * height)
                )?;
                wrote = true;
            }
            if has_rotate {
                if wrote {
                    write!(f, " ")?;
                }
                write!(
                    f,
                    "rotate({}, {}, {})",
                    Num(transform.rotate),
                    Num(cx),
                    Num(cy)
                )?;
                wrote = true;
            }
            if has_scale {
                if wrote {
                    write!(f, " ")?;
                }
                write!(
                    f,
                    "translate({}, {}) scale({}) translate({}, {})",
                    Num(cx),
                    Num(cy),
                    Num(transform.scale),
                    Num(-cx),
                    Num(-cy)
                )?;
            }
            write!(f, "\"")?;
        }
        write!(f, " href=\"#{source}-{}-{hash:08x}\"/>", variant.name)
    }

    fn write_node(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        node: &Node<'_>,
    ) -> fmt::Result {
        match node {
            Node::El { name, .. } if *name == "defs" => Ok(()),
            Node::El {
                name,
                attrs,
                children,
            } => {
                write!(f, "<{name}")?;
                self.write_attrs(f, resolver, attrs)?;
                if children.is_empty() {
                    write!(f, "/>")
                } else {
                    write!(f, ">")?;
                    for child in *children {
                        self.write_node(f, resolver, child)?;
                    }
                    write!(f, "</{name}>")
                }
            }
            Node::Component { .. } => Ok(()),
        }
    }

    #[expect(clippy::unused_self, reason = "grouped with Renderer methods")]
    fn write_attrs(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        attrs: &[(&str, AttrVal<'_>)],
    ) -> fmt::Result {
        for (key, value) in attrs {
            match value {
                AttrVal::Lit(s) => write!(f, " {key}=\"{}\"", Escaped(s))?,
                AttrVal::Color(color) => {
                    if let Some(resolved) = resolver.color(color) {
                        write!(f, " {key}=\"{}\"", Escaped(resolved))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Depth-first walk that diverts every `<defs>` element's children into
    /// the document `<defs>`, deduped by id.
    fn divert_defs<'b>(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        nodes: &[Node<'b>],
        dedup: &mut DefDedup<'b>,
    ) -> fmt::Result {
        for node in nodes {
            if let Node::El {
                name: "defs",
                children,
                ..
            } = node
            {
                for child in *children {
                    if let Some(id) = node_id(child) {
                        if dedup.seen_grad(id) {
                            continue;
                        }
                        dedup.mark_grad(id);
                    }
                    self.write_node(f, resolver, child)?;
                }
            } else if let Node::El { children, .. } = node {
                self.divert_defs(f, resolver, children, dedup)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Renderer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let resolver = Resolver::new(self.seed);
        let hash = hash_u32(self.style.source_name, self.seed);

        write!(
            f,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" fill=\"none\" \
             shape-rendering=\"auto\" aria-hidden=\"true\">",
            Num(self.style.canvas_w),
            Num(self.style.canvas_h)
        )?;
        write!(
            f,
            "<!-- Generated by DiceBear (https://www.dicebear.com) -->{}<defs>",
            self.style.metadata
        )?;

        // Pass 1: each visible component's variant body, deduped by (source, variant).
        // Embedded <defs> (gradients) are diverted into the document <defs> first.
        let mut dedup = DefDedup::new();
        for node in self.style.canvas {
            let &Node::Component {
                name,
                source,
                component,
                ..
            } = node
            else {
                continue;
            };
            let Some(variant) = resolver.variant(name, component) else {
                continue;
            };
            if dedup.seen_g(source, variant.name) {
                continue;
            }
            dedup.mark_g(source, variant.name);
            self.write_def(f, &resolver, source, variant, hash, &mut dedup)?;
        }

        // The clipPath is always applied (border radius defaults to 0).
        write!(
            f,
            "<clipPath id=\"clip-{hash:08x}\"><rect width=\"{}\" height=\"{}\" rx=\"0\" \
             ry=\"0\"/></clipPath></defs>",
            Num(self.style.canvas_w),
            Num(self.style.canvas_h)
        )?;

        let bg = resolver.color(&self.style.background).unwrap_or("");
        write!(
            f,
            "<g clip-path=\"url(#clip-{hash:08x})\"><rect width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            Num(self.style.canvas_w),
            Num(self.style.canvas_h),
            Escaped(bg)
        )?;

        // Pass 2: emit one <use> per visible component.
        for node in self.style.canvas {
            let &Node::Component {
                name,
                source,
                component,
                attrs,
            } = node
            else {
                continue;
            };
            let Some(variant) = resolver.variant(name, component) else {
                continue;
            };
            self.write_use(f, &resolver, name, source, component, variant, attrs, hash)?;
        }

        write!(f, "</g></svg>")
    }
}

/// Returns the `id` attribute of an element node, if it has a literal one.
fn node_id<'a>(node: &'a Node<'a>) -> Option<&'a str> {
    let Node::El { attrs, .. } = node else {
        return None;
    };
    attrs.iter().find_map(|(k, v)| match (*k, v) {
        ("id", AttrVal::Lit(s)) => Some(*s),
        _ => None,
    })
}
