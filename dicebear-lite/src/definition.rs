//! Typed view over the embedded constellation style definition.
//!
//! The definition is `DiceBear`'s own JSON, parsed once at startup. Attribute
//! maps keep their JSON insertion order (via `serde_json::Map` with the
//! `preserve_order` feature) because the renderer emits attributes in that
//! order.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

pub type AttrMap = serde_json::Map<String, serde_json::Value>;

/// The embedded constellation definition, parsed once.
pub static STYLE: LazyLock<StyleDef> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../constellation.json")).expect("valid constellation.json")
});

#[derive(Deserialize)]
pub struct StyleDef {
    pub canvas: CanvasDef,
    #[serde(default)]
    pub attributes: AttrMap,
    pub components: HashMap<String, ComponentDef>,
    #[serde(default)]
    pub colors: HashMap<String, ColorDef>,
    #[serde(default)]
    pub meta: MetaDef,
}

#[derive(Deserialize)]
pub struct CanvasDef {
    pub width: f64,
    pub height: f64,
    pub elements: Vec<ElementDef>,
}

/// One node in the render tree. Discriminated by `type`.
#[derive(Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ElementDef {
    Element {
        name: String,
        #[serde(default)]
        attributes: AttrMap,
        #[serde(default)]
        children: Vec<Self>,
    },
    Text {
        value: serde_json::Value,
    },
    Component {
        name: String,
        #[serde(default)]
        attributes: AttrMap,
    },
}

impl ElementDef {
    /// Returns the element's attribute map, if it has one.
    pub const fn attributes(&self) -> Option<&AttrMap> {
        match self {
            Self::Element { attributes, .. } | Self::Component { attributes, .. } => {
                Some(attributes)
            }
            Self::Text { .. } => None,
        }
    }
}

#[derive(Deserialize)]
pub struct ComponentDef {
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub probability: Option<f64>,
    #[serde(default)]
    pub translate: Option<TranslateDef>,
    #[serde(default)]
    pub rotate: Option<Range>,
    #[serde(default)]
    pub scale: Option<Range>,
    #[serde(default)]
    pub variants: HashMap<String, VariantDef>,
}

impl ComponentDef {
    /// Resolves an alias to its source component definition (one hop; chains
    /// are disallowed by the format).
    pub fn resolve<'a>(&'a self, all: &'a HashMap<String, Self>) -> &'a Self {
        match &self.extends {
            Some(source) => all.get(source).map_or(self, |c| c.resolve(all)),
            None => self,
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct TranslateDef {
    #[serde(default)]
    pub x: Option<Range>,
    #[serde(default)]
    pub y: Option<Range>,
}

#[derive(Deserialize, Clone, Copy)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

#[derive(Deserialize)]
pub struct VariantDef {
    #[serde(default)]
    pub weight: Option<f64>,
    #[serde(default)]
    pub elements: Vec<ElementDef>,
}

#[derive(Deserialize)]
pub struct ColorDef {
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Deserialize, Default)]
pub struct MetaDef {
    #[serde(default)]
    pub license: MetaParty,
    #[serde(default)]
    pub creator: MetaParty,
    #[serde(default)]
    pub source: MetaParty,
}

#[derive(Deserialize, Default)]
pub struct MetaParty {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}
