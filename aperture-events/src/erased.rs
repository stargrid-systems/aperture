//! Type-erased view of an [`EventDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many event
//! kinds together. A blanket impl bridges every [`EventDefinition`].

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

impl<T: EventDefinition> ErasedEventDefinition for T {
    fn key(&self) -> &'static str {
        T::KEY
    }

    fn payload_schema(&self) -> Value {
        json_schema::<T>()
    }
}
