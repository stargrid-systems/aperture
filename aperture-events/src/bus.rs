//! Event bus: dispatches events to subscribers and to the recorder channel.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use aperture_storage::ActorId;
use jiff::Timestamp;
use tokio::sync::mpsc;

use crate::definition::EventDefinition;
use crate::error::EventError;
use crate::payload::EventEnvelope;
use crate::stream::{EventStream, SubscriptionGuard, TypedEventStream};
use crate::subscription::Subscription;

/// Per-subscriber channel capacity.
const CHANNEL_CAPACITY: usize = 64;

/// Capacity of the channel feeding the event recorder. `emit` applies
/// backpressure once this many events are still unrecorded, so the queue
/// absorbs bursts without ever dropping events.
const RECORDER_CAPACITY: usize = 1024;

/// Central event bus for domain events.
///
/// The bus is pure in-memory: emitting hands the event to every matching
/// subscriber and queues it for the [`crate::EventRecorder`], which
/// persists batches to storage off the hot path. Cheap to clone: all clones
/// share one instance.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Inner>,
}

pub struct Inner {
    recorder_tx: mpsc::Sender<EventEnvelope>,
    recorder_rx: Mutex<Option<mpsc::Receiver<EventEnvelope>>>,
    next_subscriber_id: AtomicU64,
    subscribers: Mutex<Vec<Subscriber>>,
}

struct Subscriber {
    id: u64,
    filter: Subscription,
    sender: mpsc::Sender<EventEnvelope>,
}

impl Inner {
    pub(crate) fn remove_subscriber(&self, id: u64) {
        let mut subs = self
            .subscribers
            .lock()
            .expect("subscriber list lock poisoned");
        subs.retain(|s| s.id != id);
    }

    pub(crate) fn take_recorder(&self) -> Option<mpsc::Receiver<EventEnvelope>> {
        self.recorder_rx
            .lock()
            .expect("recorder slot lock poisoned")
            .take()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Takes the recorder channel, if no recorder has connected yet.
    pub(crate) fn take_recorder(&self) -> Option<mpsc::Receiver<EventEnvelope>> {
        self.inner.take_recorder()
    }

    /// Creates a new event bus. Persist its events by connecting an
    /// [`crate::EventRecorder`].
    pub fn new() -> Self {
        let (recorder_tx, recorder_rx) = mpsc::channel(RECORDER_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                recorder_tx,
                recorder_rx: Mutex::new(Some(recorder_rx)),
                next_subscriber_id: AtomicU64::new(0),
                subscribers: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Emits an event: queues it for recording and dispatches it to
    /// matching subscribers. No serialization happens here.
    ///
    /// Blocks while the recorder queue is full (see `RECORDER_CAPACITY`);
    /// until then emission is allocation-cheap for payloads up to 64 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::RecorderClosed`] if no recorder is draining
    /// the bus anymore.
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn emit<D: EventDefinition>(
        &self,
        payload: D,
        actor: ActorId,
    ) -> Result<EventEnvelope, EventError> {
        let envelope = EventEnvelope::new(payload, actor, Timestamp::now());
        self.inner
            .recorder_tx
            .send(envelope.clone())
            .await
            .map_err(|_| EventError::RecorderClosed)?;
        self.dispatch(&envelope);
        Ok(envelope)
    }

    /// Dispatches `event` to matching subscribers. Best-effort: a full
    /// channel drops the event and logs a warning.
    fn dispatch(&self, event: &EventEnvelope) {
        let subs = self
            .inner
            .subscribers
            .lock()
            .expect("subscriber list lock poisoned");
        for sub in subs.iter() {
            if !sub.filter.matches(event.key()) {
                continue;
            }
            if let Err(mpsc::error::TrySendError::Full(_)) = sub.sender.try_send(event.clone()) {
                tracing::warn!(
                    key = event.key(),
                    "event subscriber channel full, dropping event"
                );
            }
        }
    }

    /// Registers a subscriber with `filter` and returns its receiver + guard.
    fn add_subscriber(
        &self,
        filter: Subscription,
    ) -> (mpsc::Receiver<EventEnvelope>, SubscriptionGuard) {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let id = self
            .inner
            .next_subscriber_id
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .subscribers
            .lock()
            .expect("subscriber list lock poisoned")
            .push(Subscriber {
                id,
                filter,
                sender: tx,
            });
        let guard = SubscriptionGuard {
            inner: Arc::downgrade(&self.inner),
            id,
        };
        (rx, guard)
    }

    /// Subscribes to events matching `filter`.
    pub fn subscribe(&self, filter: Subscription) -> EventStream {
        let (rx, guard) = self.add_subscriber(filter);
        EventStream { rx, guard }
    }

    /// Subscribes to all events.
    pub fn subscribe_all(&self) -> EventStream {
        self.subscribe(Subscription::All)
    }

    /// Subscribes to events of type `D`, yielding typed payloads with full
    /// metadata. Payloads are handed over by downcast, not deserialization.
    pub fn subscribe_typed<D: EventDefinition>(&self) -> TypedEventStream<D> {
        let (rx, guard) = self.add_subscriber(Subscription::Key(D::KEY));
        TypedEventStream {
            rx,
            guard,
            _marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    use super::*;

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
    struct Probe {
        n: u32,
    }

    impl EventDefinition for Probe {
        const KEY: &'static str = "test.probe";
    }

    #[tokio::test]
    async fn typed_subscriber_receives_payload_without_serde() {
        let bus = EventBus::new();
        let mut stream = bus.subscribe_typed::<Probe>();

        let envelope = bus.emit(Probe { n: 5 }, ActorId::SYSTEM).await.unwrap();
        let event = stream.recv().await.expect("event delivered");

        assert_eq!(event.id, envelope.id);
        assert_eq!(event.key, Probe::KEY);
        assert_eq!(event.payload, Probe { n: 5 });
        assert_eq!(event.actor, ActorId::SYSTEM);
        assert_eq!(event.timestamp, envelope.timestamp);
    }

    #[tokio::test]
    async fn non_matching_subscribers_are_skipped() {
        let bus = EventBus::new();
        let mut stream = bus.subscribe(Subscription::Key("other.key"));

        bus.emit(Probe { n: 5 }, ActorId::SYSTEM).await.unwrap();
        assert!(stream.rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn raw_subscriber_gets_envelope_with_lazy_json() {
        let bus = EventBus::new();
        let mut stream = bus.subscribe_all();

        bus.emit(Probe { n: 5 }, ActorId::SYSTEM).await.unwrap();
        let envelope = stream.recv().await.expect("event delivered");
        assert_eq!(envelope.key(), Probe::KEY);
        assert_eq!(
            envelope.payload_json().unwrap(),
            serde_json::json!({ "n": 5 })
        );
    }

    #[tokio::test]
    async fn recorder_receives_every_event_in_order() {
        let bus = EventBus::new();
        let mut recorder = bus.inner.take_recorder().expect("recorder available");

        bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap();
        bus.emit(Probe { n: 2 }, ActorId::SYSTEM).await.unwrap();

        assert_eq!(
            recorder.recv().await.unwrap().payload::<Probe>().unwrap().n,
            1
        );
        assert_eq!(
            recorder.recv().await.unwrap().payload::<Probe>().unwrap().n,
            2
        );
    }

    #[tokio::test]
    async fn recorder_can_only_be_taken_once() {
        let bus = EventBus::new();
        assert!(bus.inner.take_recorder().is_some());
        assert!(bus.inner.take_recorder().is_none());
    }

    #[tokio::test]
    async fn emit_fails_once_recorder_channel_closes() {
        let bus = EventBus::new();
        drop(bus.inner.take_recorder());

        let err = bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap_err();
        assert!(matches!(err, EventError::RecorderClosed));
    }
}
