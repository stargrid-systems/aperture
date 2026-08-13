// Shared SVG data model used by style definitions and the renderer.

#[derive(Clone, Copy)]
pub struct ColorRef<'a> {
    pub key: &'a str,
    pub stops: &'a [&'a str],
}

#[derive(Clone, Copy)]
pub enum AttrVal<'a> {
    Lit(&'a str),
    Color(ColorRef<'a>),
}

pub enum Node<'a> {
    El {
        name: &'a str,
        attrs: &'a [(&'a str, AttrVal<'a>)],
        children: &'a [Self],
    },
    Component {
        name: &'a str,
        source: &'a str,
        component: &'a ComponentDef<'a>,
        attrs: &'a [(&'a str, AttrVal<'a>)],
    },
}

pub struct VariantDef<'a> {
    pub name: &'a str,
    pub weight: f64,
    pub elements: &'a [Node<'a>],
}

pub struct ComponentDef<'a> {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub probability: Option<f64>,
    pub translate: Option<((f64, f64), (f64, f64))>,
    pub rotate: Option<(f64, f64)>,
    pub scale: Option<(f64, f64)>,
    pub variants: &'a [VariantDef<'a>],
}
