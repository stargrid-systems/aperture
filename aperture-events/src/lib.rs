//! Event system for the Aperture gateway.
//!
//! A domain event is a record of a significant state change. Each event kind
//! is an [`EventDefinition`] with a unique `KEY` and a typed payload. Events
//! are emitted through the [`EventBus`], which persists them to storage and
//! broadcasts to in-process subscribers.
//!
//! The [`EventRegistry`] holds registered event definitions and projects their
//! payload schemas into the `OpenAPI` document.

pub use aperture_storage::{Event, EventFilter, EventId, EventRepository, NewEvent};

pub use self::bus::EventBus;
pub use self::definition::EventDefinition;
pub use self::erased::ErasedEventDefinition;
pub use self::error::EventError;
pub use self::registry::{EventDescriptor, EventRegistry};

mod bus;
mod definition;
mod erased;
mod error;
mod registry;
