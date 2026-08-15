//! The registry of task definitions.
//!
//! Definitions are registered once at startup. The registry is the single
//! source of truth for what kinds exist: [`TaskRegistry`] looks up definitions
//! by key, and the HTTP layer projects [`TaskRegistry::descriptors`] into the
//! `OpenAPI` document.

use std::collections::HashMap;
use std::sync::Arc;

use utoipa::openapi::{RefOr, Schema};

use crate::definition::{Capabilities, TaskDefinition};
use crate::erased::ErasedDefinition;

/// A public, schema-carrying description of one registered key.
pub struct TaskDescriptor {
    /// The key string.
    pub key: &'static str,
    /// What the key supports.
    pub capabilities: Capabilities,
    /// Component name of the key's input type.
    pub input_name: String,
    /// Component name of the key's output type.
    pub output_name: String,
    /// Schema of the key's input type.
    pub input_schema: RefOr<Schema>,
    /// Schema of the key's output type.
    pub output_schema: RefOr<Schema>,
    /// Named component schemas the input and output reference, including
    /// themselves and their dependencies.
    pub schemas: Vec<(String, RefOr<Schema>)>,
}

/// A registry of task definitions, keyed by definition key.
#[derive(Default)]
pub struct TaskRegistry {
    definitions: HashMap<&'static str, Arc<dyn ErasedDefinition>>,
}

impl TaskRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `definition` under its [`TaskDefinition::KEY`], replacing any
    /// previously registered definition for that key.
    pub fn register<T: TaskDefinition>(&mut self, definition: T) {
        self.definitions.insert(T::KEY, Arc::new(definition));
    }

    /// A schema-carrying descriptor per registered key.
    pub fn descriptors(&self) -> Vec<TaskDescriptor> {
        self.definitions
            .values()
            .map(|definition| {
                let mut schemas = Vec::new();
                definition.collect_schemas(&mut schemas);
                TaskDescriptor {
                    key: definition.key(),
                    capabilities: definition.capabilities(),
                    input_name: definition.input_name(),
                    output_name: definition.output_name(),
                    input_schema: definition.input_schema(),
                    output_schema: definition.output_schema(),
                    schemas,
                }
            })
            .collect()
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Arc<dyn ErasedDefinition>> {
        self.definitions.get(key)
    }
}
