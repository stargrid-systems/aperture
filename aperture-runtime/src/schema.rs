//! Standalone JSON Schema documents derived from `OpenAPI` components.

use std::collections::HashSet;

use serde_json::Value;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::Schema;
use utoipa::{PartialSchema, ToSchema};

const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
const COMPONENT_PREFIX: &str = "#/components/schemas/";

/// Builds a standalone JSON Schema document for `T`.
///
/// The document targets draft 2020-12: `T`'s own schema at the root, its
/// dependencies under `$defs`, and every internal `$ref` rewritten from
/// `#/components/schemas/...` to `#/$defs/...`. `$defs` is omitted when `T`
/// has no dependencies.
///
/// # Panics
///
/// Panics if a component schema fails to serialize, which indicates a bug in
/// the `ToSchema` implementation.
pub fn json_schema<T: ToSchema>() -> Value {
    let mut collected: Vec<(String, RefOr<Schema>)> = Vec::new();
    collected.push((
        <T as ToSchema>::name().into_owned(),
        <T as PartialSchema>::schema(),
    ));
    <T as ToSchema>::schemas(&mut collected);
    let own_name = <T as ToSchema>::name().into_owned();

    let known: HashSet<String> = collected.iter().map(|(name, _)| name.clone()).collect();
    let mut doc =
        serde_json::to_value(<T as PartialSchema>::schema()).expect("schema must serialize");
    rewrite_refs(&mut doc, &known);

    let mut defs = serde_json::Map::new();
    for (name, schema) in collected {
        if name == own_name || defs.contains_key(&name) {
            continue;
        }
        let mut value = serde_json::to_value(schema).expect("schema must serialize");
        rewrite_refs(&mut value, &known);
        defs.insert(name, value);
    }
    if !defs.is_empty() {
        doc["$defs"] = Value::Object(defs);
    }
    doc["$schema"] = Value::String(DIALECT.to_owned());
    doc
}

/// Rewrites every `$ref` that points at a collected component to `$defs`.
fn rewrite_refs(value: &mut Value, known: &HashSet<String>) {
    match value {
        Value::Object(fields) => {
            for (name, field) in fields.iter_mut() {
                if name == "$ref"
                    && let Value::String(reference) = field
                    && let Some(component) = reference.strip_prefix(COMPONENT_PREFIX)
                    && known.contains(component)
                {
                    *reference = format!("#/$defs/{component}");
                } else {
                    rewrite_refs(field, known);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                rewrite_refs(item, known);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use utoipa::ToSchema;

    use super::*;

    #[derive(ToSchema)]
    #[expect(dead_code, reason = "only used for its schema")]
    struct Source {
        reference: String,
    }

    #[derive(ToSchema)]
    #[expect(dead_code, reason = "only used for its schema")]
    struct Input {
        source: Source,
    }

    #[test]
    fn emits_dialect_and_no_defs_without_dependencies() {
        #[derive(ToSchema)]
        #[expect(dead_code, reason = "only used for its schema")]
        struct Plain {
            n: u32,
        }

        let doc = json_schema::<Plain>();
        assert_eq!(doc["$schema"], DIALECT);
        assert_eq!(doc["type"], "object");
        assert!(doc.get("$defs").is_none());
    }

    #[test]
    fn rewrites_refs_into_defs() {
        let doc = json_schema::<Input>();
        assert_eq!(doc["$schema"], DIALECT);
        assert_eq!(
            doc["properties"]["source"]["$ref"], "#/$defs/Source",
            "root refs point at $defs: {doc}"
        );
        assert_eq!(doc["$defs"]["Source"]["type"], "object");
    }
}
