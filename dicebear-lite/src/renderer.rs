//! SVG serializer. Walks the canvas element tree and streams markup
//! byte-for-byte compatible with `DiceBear`'s `Renderer` for default options,
//! without allocating.
//!
//! Output order is `<defs>` (component variant bodies deduped, then the
//! clipPath) followed by the clipped body (background plus `<use>` references).
//! Because `<defs>` must precede the body but is discovered while walking it,
//! the renderer makes two deterministic passes: variant selection is fully
//! keyed by `(seed, key)`, so recomputing it is safe.

use core::cell::Cell;
use core::fmt;

use crate::Style;
use crate::data::{AttrVal, ComponentDef, Node};
use crate::number::{Num, equals};
use crate::prng::hash_u32;
use crate::resolver::Resolver;
use crate::xml::Escaped;

const MAX_DEFS: usize = 16;

#[expect(
    clippy::declare_interior_mutable_const,
    reason = "Cell is required for interior mutability in no_std Display impl"
)]
const NONE_G: Cell<Option<(&'static str, &'static str)>> = Cell::new(None);
#[expect(
    clippy::declare_interior_mutable_const,
    reason = "Cell is required for interior mutability in no_std Display impl"
)]
const NONE_GRAD: Cell<Option<&'static str>> = Cell::new(None);

/// Interior-mutable dedup state for pass 1. Uses `Cell` so the renderer can
/// mutate it through `&self` inside a `Display` impl.
struct DefDedup {
    seen_g: [Cell<Option<(&'static str, &'static str)>>; MAX_DEFS],
    seen_grad: [Cell<Option<&'static str>>; MAX_DEFS],
}

impl DefDedup {
    const fn new() -> Self {
        Self {
            seen_g: [NONE_G; MAX_DEFS],
            seen_grad: [NONE_GRAD; MAX_DEFS],
        }
    }

    fn seen_g(&self, source: &str, variant: &str) -> bool {
        self.seen_g
            .iter()
            .any(|slot| slot.get().is_some_and(|(s, v)| s == source && v == variant))
    }

    fn mark_g(&self, source: &'static str, variant: &'static str) {
        for slot in &self.seen_g {
            if slot.get().is_none() {
                slot.set(Some((source, variant)));
                return;
            }
        }
    }

    fn seen_grad(&self, id: &str) -> bool {
        self.seen_grad
            .iter()
            .any(|slot| slot.get().is_some_and(|s| s == id))
    }

    fn mark_grad(&self, id: &'static str) {
        for slot in &self.seen_grad {
            if slot.get().is_none() {
                slot.set(Some(id));
                return;
            }
        }
    }
}

pub struct Renderer<'a> {
    seed: &'a str,
    style: &'static Style,
}

impl<'a> Renderer<'a> {
    pub const fn new(seed: &'a str, style: &'static Style) -> Self {
        Self { seed, style }
    }

    /// Writes a component's def: embedded `<defs>` (gradients) are diverted
    /// into the document `<defs>` (deduped by id), then the variant body is
    /// wrapped in `<g id="...">`.
    #[expect(clippy::too_many_arguments, reason = "rendering parameters")]
    fn write_def(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        component: &'static ComponentDef,
        source: &'static str,
        variant: &'static str,
        hash: u32,
        dedup: &DefDedup,
    ) -> fmt::Result {
        let base = self.style.resolve(component);
        let Some((_, def)) = base.variants.iter().find(|(n, _)| n == &variant) else {
            return Ok(());
        };

        self.divert_defs(f, resolver, def.elements, dedup)?;

        write!(f, "<g id=\"{source}-{variant}-{hash:08x}\">")?;
        for el in def.elements {
            if !matches!(el, Node::El { name: "defs", .. }) {
                self.write_node(f, resolver, el)?;
            }
        }
        write!(f, "</g>")
    }

    /// Writes a `<use>` reference with the merged placement + component
    /// transform.
    #[expect(clippy::too_many_arguments, reason = "rendering parameters")]
    fn write_use(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        name: &'static str,
        component: &'static ComponentDef,
        variant: &'static str,
        attrs: &[(&'static str, AttrVal)],
        hash: u32,
    ) -> fmt::Result {
        let source = component.extends.unwrap_or(name);
        let base = self.style.resolve(component);
        let transform = resolver.component_transform(name);
        let width = base.width.unwrap_or(0.0);
        let height = base.height.unwrap_or(0.0);
        let cx = width / 2.0;
        let cy = height / 2.0;

        let user_transform = attrs.iter().find_map(|(k, v)| {
            if *k == "transform"
                && let AttrVal::Lit(s) = v
            {
                return Some(*s);
            }
            None
        });
        let has_translate =
            !equals(transform.translate_x, 0.0) || !equals(transform.translate_y, 0.0);
        let has_rotate = !equals(transform.rotate, 0.0);
        let has_scale = !equals(transform.scale, 1.0);

        write!(f, "<use")?;
        if user_transform.is_some() || has_translate || has_rotate || has_scale {
            f.write_str(" transform=\"")?;
            let mut wrote = user_transform.is_some();
            if let Some(ut) = user_transform {
                f.write_str(ut)?;
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
            f.write_str("\"")?;
        }
        write!(f, " href=\"#{source}-{variant}-{hash:08x}\"/>")
    }

    fn write_node(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        node: &Node,
    ) -> fmt::Result {
        if let Node::El { name: "defs", .. } = node {
            return Ok(());
        }
        match node {
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
        attrs: &[(&'static str, AttrVal)],
    ) -> fmt::Result {
        for (key, value) in attrs {
            let s = match value {
                AttrVal::Lit(s) => *s,
                AttrVal::Color(name) => resolver.color(name),
            };
            write!(f, " {key}=\"{}\"", Escaped(s))?;
        }
        Ok(())
    }

    /// Depth-first walk that diverts every `<defs>` element's children into
    /// the document `<defs>`, deduped by id.
    fn divert_defs(
        &self,
        f: &mut fmt::Formatter<'_>,
        resolver: &Resolver<'_>,
        nodes: &[Node],
        dedup: &DefDedup,
    ) -> fmt::Result {
        for node in nodes {
            if let Node::El {
                name: "defs",
                children,
                ..
            } = node
            {
                for child in *children {
                    let id = node_id(child);
                    if id.is_some_and(|i| dedup.seen_grad(i)) {
                        continue;
                    }
                    if let Some(i) = id {
                        dedup.mark_grad(i);
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
        let resolver = Resolver::new(self.seed, self.style);
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
        let dedup = DefDedup::new();
        for node in self.style.canvas {
            let &Node::Component { name, .. } = node else {
                continue;
            };
            let Some(variant) = resolver.variant(name) else {
                continue;
            };
            let component = self
                .style
                .component(name)
                .expect("canvas references a known component");
            let source = component.extends.unwrap_or(name);
            if dedup.seen_g(source, variant) {
                continue;
            }
            dedup.mark_g(source, variant);
            self.write_def(f, &resolver, component, source, variant, hash, &dedup)?;
        }

        // The clipPath is always applied (border radius defaults to 0).
        write!(
            f,
            "<clipPath id=\"clip-{hash:08x}\"><rect width=\"{}\" height=\"{}\" rx=\"0\" \
             ry=\"0\"/></clipPath></defs>",
            Num(self.style.canvas_w),
            Num(self.style.canvas_h)
        )?;

        write!(
            f,
            "<g clip-path=\"url(#clip-{hash:08x})\"><rect width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            Num(self.style.canvas_w),
            Num(self.style.canvas_h),
            Escaped(resolver.color("background"))
        )?;

        // Pass 2: emit one <use> per visible component.
        for node in self.style.canvas {
            let &Node::Component { name, attrs } = node else {
                continue;
            };
            let Some(variant) = resolver.variant(name) else {
                continue;
            };
            let component = self
                .style
                .component(name)
                .expect("canvas references a known component");
            self.write_use(f, &resolver, name, component, variant, attrs, hash)?;
        }

        write!(f, "</g></svg>")
    }
}

/// Returns the `id` attribute of an element node, if it has a literal one.
fn node_id(node: &Node) -> Option<&'static str> {
    let Node::El { attrs, .. } = node else {
        return None;
    };
    for &(key, value) in *attrs {
        if key == "id"
            && let AttrVal::Lit(s) = value
        {
            return Some(s);
        }
    }
    None
}
