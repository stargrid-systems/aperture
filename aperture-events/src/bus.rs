//! Event bus: persists events and dispatches to filtered subscribers.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::marker::PhantomData;

use aperture_storage::{ActorId, Event, EventRepository, NewEvent};
use jiff::Timestamp;
use tokio::sync::mpsc;

use crate::definition::EventDefinition;
use crate::error::EventError;
use crate::stream::{EventStream, SubscriptionGuard, TypedEventStream};
use crate::subscription::Subscription;

/// Per-subscriber channel capacity.
const CHANNEL_CAPACITY: usize = 64;

/// Central event bus for domain events.
///
/// Persists every emitted event to storage and dispatches to subscribers
/// whose [`Subscription`] matches. Cheap to clone: all clones share one
/// instance.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Inner>,
}

pub struct Inner {
    repo: EventRepository,
    next_id: AtomicU64,
    subscribers: Mutex<Vec<Subscriber>>,
}

struct Subscriber {
    id: u64,
    filter: Subscription,
    sender: mpsc::Sender<Event>,
}

impl Inner {
    pub(crate) fn remove_subscriber(&self, id: u64) {
        let mut subs = self
            .subscribers
            .lock()
            .expect("subscriber list lock poisoned");
        subs.retain(|s| s.id != id);
    }
}

impl EventBus {
    /// Creates a new event bus backed by `repo`.
    pub fn new(repo: EventRepository) -> Self {
        Self {
            inner: Arc::new(Inner {
                repo,
                next_id: AtomicU64::new(0),
                subscribers: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Emits an event: serializes the payload, persists it, and dispatches
    /// to matching subscribers.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::Serialize`] if the payload cannot be serialized,
    /// or [`EventError::Storage`] if persistence fails.
    #[tracing::instrument(level = "info", skip(self, payload))]
    pub async fn emit<D: EventDefinition>(
        &self,
        payload: D,
        actor: ActorId,
    ) -> Result<Event, EventError> {
        let data = serde_json::to_value(&payload).map_err(EventError::Serialize)?;
        let now = Timestamp::now();
        let id = self
            .inner
            .repo
            .create(&NewEvent {
                key: D::KEY.to_owned(),
                data: data.clone(),
                actor,
                timestamp: now,
            })
            .await?;
        let event = Event {
            id,
            key: D::KEY.to_owned(),
            data,
            actor,
            timestamp: now,
        };

        self.dispatch(&event);

        Ok(event)
    }

    /// Dispatches `event` to matching subscribers. Best-effort: a full
    /// channel drops the event and logs a warning.
    fn dispatch(&self, event: &Event) {
        let subs = self
            .inner
            .subscribers
            .lock()
            .expect("subscriber list lock poisoned");
        for sub in subs.iter() {
            if !sub.filter.matches(&event.key) {
                continue;
            }
            if let Err(mpsc::error::TrySendError::Full(_)) = sub.sender.try_send(event.clone()) {
                tracing::warn!(
                    key = %event.key,
                    "event subscriber channel full, dropping event"
                );
            }
        }
    }

    /// Registers a subscriber with `filter` and returns its receiver + guard.
    fn add_subscriber(
        &self,
        filter: Subscription,
    ) -> (
        mpsc::Receiver<Event>,
        SubscriptionGuard,
    ) {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
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

    /// Subscribes to events of type `D`, yielding decoded payloads with full
    /// metadata.
    pub fn subscribe_typed<D: EventDefinition>(&self) -> TypedEventStream<D> {
        let (rx, guard) = self.add_subscriber(Subscription::Key(D::KEY));
        TypedEventStream {
            rx,
            guard,
            _marker: PhantomData,
        }
    }
}
