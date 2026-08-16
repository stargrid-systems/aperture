//! Event system for the Aperture gateway.
//!
//! A domain event is a record of a significant state change. Each event kind
//! is an [`EventDefinition`] with a unique `KEY` and a typed payload. Events
//! are emitted through the [`EventBus`], which dispatches to in-process
//! subscribers and queues every event for the [`EventRecorder`], the worker
//! that persists batches to storage off the emit path.
//!
//! The bus acts as a topic-filtered broker. Subscribers register a
//! [`Subscription`] filter and only receive matching events, so independent
//! consumers are not woken up by unrelated event kinds. For type-safe
//! access, [`EventBus::subscribe_typed`] returns a [`TypedEventStream`]
//! that yields the payload itself: payloads travel type-erased (up to 64
//! bytes inline) and are recovered by downcast, so the dispatch path never
//! serializes. JSON exists only where a consumer asks for it, via
//! [`EventEnvelope::payload_json`].
//!
//! The [`EventRegistry`] holds the registered event kinds and serves their
//! payload schemas.

pub use aperture_storage::{Event, EventFilter, EventId, EventRepository, NewEvent};

pub use self::bus::EventBus;
pub use self::definition::EventDefinition;
pub use self::erased::ErasedEventDefinition;
pub use self::error::EventError;
pub use self::payload::EventEnvelope;
pub use self::recorder::EventRecorder;
pub use self::stream::{EventStream, TypedEvent, TypedEventStream};
pub use self::subscription::Subscription;

mod bus;
mod definition;
mod erased;
mod error;
mod payload;
mod recorder;
mod stream;
mod subscription;

/// The registry of event definitions, keyed by event key.
pub type EventRegistry = aperture_runtime::Registry<dyn ErasedEventDefinition>;
