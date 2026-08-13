// Shared SVG data model used by style definitions and the renderer.

#[derive(Clone, Copy)]
pub enum AttrVal {
    Lit(&'static str),
    Color(&'static str),
}

pub enum Node {
    El {
        name: &'static str,
        attrs: &'static [(&'static str, AttrVal)],
        children: &'static [Self],
    },
    Component {
        name: &'static str,
        attrs: &'static [(&'static str, AttrVal)],
    },
}

pub struct VariantDef {
    pub weight: f64,
    pub elements: &'static [Node],
}

pub struct ComponentDef {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub probability: Option<f64>,
    pub translate: Option<((f64, f64), (f64, f64))>,
    pub rotate: Option<(f64, f64)>,
    pub scale: Option<(f64, f64)>,
    pub extends: Option<&'static str>,
    pub variants: &'static [(&'static str, VariantDef)],
}
