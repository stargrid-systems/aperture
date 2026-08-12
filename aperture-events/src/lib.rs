//! Event system for the Aperture gateway.
//!
//! A domain event is a record of a significant state change. Each event kind
//! is an [`EventDefinition`] with a unique `KEY` and a typed payload. Events
//! are emitted through the [`EventBus`], which persists them to storage and
//! dispatches to in-process subscribers.
//!
//! The bus acts as a topic-filtered broker. Subscribers register a
//! [`Subscription`] filter and only receive matching events, so independent
//! consumers are not woken up by unrelated event kinds. For type-safe
//! access, [`EventBus::subscribe_typed`] returns a [`TypedEventStream`] that
//! yields decoded payloads.
//!
//! The [`EventRegistry`] holds registered event definitions and projects their
//! payload schemas into the `OpenAPI` document.

pub use aperture_storage::{Event, EventFilter, EventId, EventRepository, NewEvent};

pub use self::bus::EventBus;
pub use self::definition::EventDefinition;
pub use self::erased::ErasedEventDefinition;
pub use self::error::EventError;
pub use self::registry::{EventDescriptor, EventRegistry};
pub use self::stream::{EventStream, TypedEvent, TypedEventStream};
pub use self::subscription::Subscription;

mod bus;
mod definition;
mod erased;
mod error;
mod registry;
mod stream;
mod subscription;
