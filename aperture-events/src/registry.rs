//! The registry of event definitions.

use std::sync::Arc;

use aperture_storage::Registry;
use utoipa::openapi::schema::Schema;
use utoipa::openapi::RefOr;

use crate::definition::EventDefinition;
use crate::erased::ErasedEventDefinition;

/// A public, schema-carrying description of one registered event kind.
pub struct EventDescriptor {
    /// The key string.
    pub key: &'static str,
    /// Component name of the event's payload type.
    pub payload_name: String,
    /// Schema of the event's payload type.
    pub payload_schema: RefOr<Schema>,
    /// Named component schemas the payload references, including itself and
    /// its dependencies.
    pub schemas: Vec<(String, RefOr<Schema>)>,
}

/// A registry of event definitions, keyed by event key.
#[derive(Default)]
pub struct EventRegistry(Registry<dyn ErasedEventDefinition>);

impl EventRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `definition` under its [`EventDefinition::KEY`], replacing
    /// any previously registered definition for that key.
    pub fn register<T: EventDefinition>(&mut self, definition: T) {
        self.0.register(T::KEY, Arc::new(definition));
    }

    /// A schema-carrying descriptor per registered key.
    pub fn descriptors(&self) -> impl Iterator<Item = EventDescriptor> + '_ {
        self.0.values().map(|definition| {
            let mut schemas = Vec::new();
            definition.collect_schemas(&mut schemas);
            EventDescriptor {
                key: definition.key(),
                payload_name: definition.payload_name(),
                payload_schema: definition.payload_schema(),
                schemas,
            }
        })
    }

    /// Iterates over the registered keys.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.keys()
    }
}
