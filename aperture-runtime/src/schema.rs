//! Standalone JSON Schema documents derived from `OpenAPI` components.

use std::collections::{BTreeMap, HashSet};

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
/// has no dependencies. `T` itself appears in `$defs` when the document
/// references it, so self-referential types resolve.
///
/// # Panics
///
/// Panics if a component schema fails to serialize (a bug in the `ToSchema`
/// implementation), if two dependencies register conflicting schemas under
/// the same component name, or if a `$ref` points at an unknown component
/// and would dangle.
pub fn json_schema<T: ToSchema>() -> Value {
    let own_name = <T as ToSchema>::name().into_owned();
    let mut collected: Vec<(String, RefOr<Schema>)> = Vec::new();
    <T as ToSchema>::schemas(&mut collected);

    let mut components: BTreeMap<String, RefOr<Schema>> = BTreeMap::new();
    // The type's own entry must be the full schema from the root, not the
    // bare `Ref` that `schemas` legitimately emits for a recursive type.
    components.insert(own_name.clone(), <T as PartialSchema>::schema());
    for (name, schema) in collected {
        if name == own_name {
            continue;
        }
        match components.get(&name) {
            // The same name collected twice with the same shape (a common
            // type) is fine. A different shape under the same name would
            // silently corrupt the document, so fail loudly.
            Some(existing) => assert!(
                serde_json::to_value(existing).expect("schema must serialize")
                    == serde_json::to_value(&schema).expect("schema must serialize"),
                "conflicting schemas for component {name:?}"
            ),
            None => {
                components.insert(name, schema);
            }
        }
    }

    let mut referenced: HashSet<String> = HashSet::new();
    let mut doc =
        serde_json::to_value(<T as PartialSchema>::schema()).expect("schema must serialize");
    rewrite_refs(&mut doc, &components, &mut referenced);

    let mut defs = serde_json::Map::new();
    for name in components.keys() {
        if name != &own_name {
            push_def(&mut defs, &components, name, &mut referenced);
        }
    }
    // A self-referential type resolves only if it is present in `$defs`.
    if referenced.contains(&own_name) {
        push_def(&mut defs, &components, &own_name, &mut referenced);
    }
    if !defs.is_empty() {
        doc["$defs"] = Value::Object(defs);
    }
    doc["$schema"] = Value::String(DIALECT.to_owned());
    doc
}

/// Serializes `name`'s component, rewrites its refs, and adds it to `defs`.
fn push_def(
    defs: &mut serde_json::Map<String, Value>,
    components: &BTreeMap<String, RefOr<Schema>>,
    name: &str,
    referenced: &mut HashSet<String>,
) {
    if defs.contains_key(name) {
        return;
    }
    let mut value = serde_json::to_value(&components[name]).expect("schema must serialize");
    rewrite_refs(&mut value, components, referenced);
    defs.insert(name.to_owned(), value);
}

/// Rewrites every `$ref` that points at a known component to `$defs` and
/// records it in `referenced`.
fn rewrite_refs(
    value: &mut Value,
    components: &BTreeMap<String, RefOr<Schema>>,
    referenced: &mut HashSet<String>,
) {
    match value {
        Value::Object(fields) => {
            for (name, field) in fields.iter_mut() {
                if name == "$ref"
                    && let Value::String(reference) = field
                    && let Some(component) =
                        reference.strip_prefix(COMPONENT_PREFIX).map(str::to_owned)
                {
                    assert!(
                        components.contains_key(&component),
                        "schema references unknown component {component:?}"
                    );
                    *reference = format!("#/$defs/{component}");
                    referenced.insert(component);
                } else {
                    rewrite_refs(field, components, referenced);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                rewrite_refs(item, components, referenced);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use utoipa::ToSchema;
    use utoipa::openapi::Ref;
    use utoipa::openapi::RefOr;
    use utoipa::openapi::schema::{ObjectBuilder, Schema, Type};

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

    /// A self-referential type with a hand-written schema, standing in for
    /// what a recursive plugin type produces.
    struct Node;

    impl ToSchema for Node {
        fn name() -> Cow<'static, str> {
            "Node".into()
        }

        fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
            schemas.push((
                "Node".to_owned(),
                RefOr::Ref(Ref::from_schema_name("Node")),
            ));
        }
    }

    impl PartialSchema for Node {
        fn schema() -> RefOr<Schema> {
            let next = ObjectBuilder::new()
                .schema_type(Type::Object)
                .property("next", Ref::from_schema_name("Node"))
                .required("next")
                .build();
            RefOr::T(Schema::Object(next))
        }
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

    #[test]
    fn includes_the_type_itself_when_self_referenced() {
        let doc = json_schema::<Node>();
        assert_eq!(doc["properties"]["next"]["$ref"], "#/$defs/Node");
        assert_eq!(
            doc["$defs"]["Node"]["properties"]["next"]["$ref"], "#/$defs/Node",
            "self-reference resolves inside $defs: {doc}"
        );
    }
}
