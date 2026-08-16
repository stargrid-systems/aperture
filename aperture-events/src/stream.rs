//! Subscriber-owned event streams and typed event views.

use std::marker::PhantomData;
use std::sync::Weak;

use aperture_storage::{ActorId, EventId};
use jiff::Timestamp;
use tokio::sync::mpsc;

use crate::bus::Inner;
use crate::definition::EventDefinition;
use crate::payload::EventEnvelope;

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
/// Yields the type-erased [`EventEnvelope`]. Call
/// [`EventEnvelope::payload_json`] only if the JSON form is needed.
///
/// Cleans up its subscriber entry when dropped.
pub struct EventStream {
    pub(crate) rx: mpsc::Receiver<EventEnvelope>,
    #[expect(dead_code)]
    pub(crate) guard: SubscriptionGuard,
}

impl EventStream {
    /// Receives the next matching event.
    ///
    /// Returns `None` when the event bus has been dropped.
    pub async fn recv(&mut self) -> Option<EventEnvelope> {
        self.rx.recv().await
    }
}

/// A live stream of typed events.
///
/// Subscribers receive the payload itself with full metadata. The broker
/// filter ensures only events with key `D::KEY` are dispatched, and the
/// payload is recovered by downcast, so the dispatch path never
/// deserializes.
///
/// Cleans up its subscriber entry when dropped.
pub struct TypedEventStream<D: EventDefinition> {
    pub(crate) rx: mpsc::Receiver<EventEnvelope>,
    #[expect(dead_code)]
    pub(crate) guard: SubscriptionGuard,
    pub(crate) _marker: PhantomData<D>,
}

impl<D: EventDefinition> TypedEventStream<D> {
    /// Receives the next event as a typed payload with metadata.
    ///
    /// Returns `None` when the event bus has been dropped.
    ///
    /// If the payload fails to downcast (which cannot happen while event
    /// keys are unique per type), a warning is logged and the next event is
    /// awaited.
    pub async fn recv(&mut self) -> Option<TypedEvent<D>> {
        loop {
            let event = self.rx.recv().await?;
            match event.payload::<D>() {
                Some(payload) => {
                    return Some(TypedEvent {
                        id: event.id,
                        key: D::KEY,
                        payload: payload.clone(),
                        actor: event.actor,
                        timestamp: event.timestamp,
                    });
                }
                None => {
                    tracing::warn!(
                        key = event.key(),
                        "failed to downcast event payload, skipping"
                    );
                }
            }
        }
    }
}

/// A typed event with full envelope metadata.
#[derive(Debug, Clone)]
pub struct TypedEvent<D> {
    /// Unique id of the event, assigned at emit time.
    pub id: EventId,
    /// The event key (= `D::KEY`).
    pub key: &'static str,
    /// The typed payload.
    pub payload: D,
    /// Actor that triggered the event.
    pub actor: ActorId,
    /// When the event was emitted.
    pub timestamp: Timestamp,
}
