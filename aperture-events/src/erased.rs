//! Type-erased view of an [`EventDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many event
//! kinds together. A blanket impl bridges every [`EventDefinition`].

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use aperture_runtime::{RegistryEntry, json_schema};
use serde_json::Value;

use crate::definition::EventDefinition;

pub trait ErasedEventDefinition: Send + Sync + 'static {
    /// The key this definition is registered under.
    fn key(&self) -> &'static str;
    /// A standalone JSON Schema document of the event's payload type.
    fn payload_schema(&self) -> Value;
}

impl RegistryEntry for dyn ErasedEventDefinition {
    fn key(&self) -> &'static str {
        ErasedEventDefinition::key(self)
    }
}

/// Schemas cached per event kind. Statics declared in generic scopes are
/// shared across all monomorphizations, so the cache is keyed by [`TypeId`]
/// instead of relying on per-instantiation statics.
fn schema_cache() -> &'static RwLock<HashMap<TypeId, Arc<Value>>> {
    static CACHE: OnceLock<RwLock<HashMap<TypeId, Arc<Value>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

impl<T: EventDefinition> ErasedEventDefinition for T {
    fn key(&self) -> &'static str {
        T::KEY
    }

    fn payload_schema(&self) -> Value {
        let id = TypeId::of::<T>();
        if let Some(schema) = schema_cache()
            .read()
            .expect("schema cache lock poisoned")
            .get(&id)
        {
            return schema.as_ref().clone();
        }
        let schema = Arc::new(json_schema::<T>());
        let mut cache = schema_cache().write().expect("schema cache lock poisoned");
        let cached = cache.entry(id).or_insert_with(|| schema);
        cached.as_ref().clone()
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    use super::*;

    #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
    struct KindA {
        a: u32,
    }

    impl EventDefinition for KindA {
        const KEY: &'static str = "test.kind_a";
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
    struct KindB {
        b: String,
    }

    impl EventDefinition for KindB {
        const KEY: &'static str = "test.kind_b";
    }

    #[test]
    fn cached_schemas_stay_per_kind() {
        let a: &dyn ErasedEventDefinition = &KindA::default();
        let b: &dyn ErasedEventDefinition = &KindB::default();
        assert_eq!(a.payload_schema(), json_schema::<KindA>());
        assert_eq!(b.payload_schema(), json_schema::<KindB>());
        // Repeated reads must keep serving each kind its own document.
        assert_ne!(a.payload_schema(), b.payload_schema());
    }
}
