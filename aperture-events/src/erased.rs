//! Type-erased view of an [`EventDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many event
//! kinds together. A blanket impl bridges every [`EventDefinition`].

use aperture_runtime::RegistryEntry;

use crate::definition::EventDefinition;

pub trait ErasedEventDefinition: Send + Sync + 'static {
    /// The key this definition is registered under.
    fn key(&self) -> &'static str;
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
}
