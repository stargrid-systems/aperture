//! Subscriber-owned event streams and typed event envelopes.

use std::marker::PhantomData;
use std::sync::Weak;

use aperture_storage::{ActorId, Event, EventId};
use jiff::Timestamp;
use tokio::sync::mpsc;

use crate::bus::Inner;
use crate::definition::EventDefinition;

/// Removes the subscriber entry from the bus when dropped.
pub struct SubscriptionGuard {
    pub(crate) inner: Weak<Inner>,
    pub(crate) id: u64,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.remove_subscriber(self.id);
        }
    }
}

/// A live stream of raw events matching a [`super::Subscription`].
///
/// Cleans up its subscriber entry when dropped.
pub struct EventStream {
    pub(crate) rx: mpsc::Receiver<Event>,
    #[expect(dead_code)]
    pub(crate) guard: SubscriptionGuard,
}

impl EventStream {
    /// Receives the next matching event.
    ///
    /// Returns `None` when the event bus has been dropped.
    pub async fn recv(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

/// A live stream of typed events.
///
/// Subscribers receive decoded payloads with full metadata. The underlying
/// broker filter ensures only events with key `D::KEY` are dispatched.
///
/// Cleans up its subscriber entry when dropped.
pub struct TypedEventStream<D: EventDefinition> {
    pub(crate) rx: mpsc::Receiver<Event>,
    #[expect(dead_code)]
    pub(crate) guard: SubscriptionGuard,
    pub(crate) _marker: PhantomData<D>,
}

impl<D: EventDefinition> TypedEventStream<D> {
    /// Receives the next event as a decoded payload with metadata.
    ///
    /// Returns `None` when the event bus has been dropped.
    ///
    /// If the event payload fails to decode (which should not happen under
    /// normal operation), a warning is logged and the next event is awaited.
    pub async fn recv(&mut self) -> Option<TypedEvent<D>> {
        loop {
            let event = self.rx.recv().await?;
            match serde_json::from_value::<D>(event.data) {
                Ok(payload) => {
                    return Some(TypedEvent {
                        id: event.id,
                        key: D::KEY,
                        payload,
                        actor: event.actor,
                        timestamp: event.timestamp,
                    });
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        key = %event.key,
                        "failed to decode event payload, skipping"
                    );
                }
            }
        }
    }
}

/// A typed event with full envelope metadata.
#[derive(Debug, Clone)]
pub struct TypedEvent<D> {
    /// Unique id of the persisted event row.
    pub id: EventId,
    /// The event key (= `D::KEY`).
    pub key: &'static str,
    /// The decoded payload.
    pub payload: D,
    /// Actor that triggered the event.
    pub actor: ActorId,
    /// When the event was emitted.
    pub timestamp: Timestamp,
}
