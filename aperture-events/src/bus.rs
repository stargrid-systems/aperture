//! Event bus: persists events and broadcasts to in-process subscribers.

use std::sync::Arc;

use aperture_storage::{ActorId, Event, EventRepository, NewEvent};
use jiff::Timestamp;
use tokio::sync::broadcast;

use crate::definition::EventDefinition;
use crate::error::EventError;

const CHANNEL_CAPACITY: usize = 64;

/// Central event bus for domain events.
///
/// Persists every emitted event to storage and broadcasts to subscribers.
/// Cheap to clone: all clones share one instance.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Inner>,
}

struct Inner {
    repo: EventRepository,
    broadcast: broadcast::Sender<Event>,
}

impl EventBus {
    /// Creates a new event bus backed by `repo`.
    pub fn new(repo: EventRepository) -> Self {
        let (broadcast, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(Inner { repo, broadcast }),
        }
    }

    /// Emits an event: serializes the payload, persists it, and broadcasts
    /// to subscribers.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::Serialize`] if the payload cannot be serialized,
    /// or [`EventError::Storage`] if the persistence fails.
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
        let _ = self.inner.broadcast.send(event.clone());
        Ok(event)
    }

    /// Subscribes to the live event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.broadcast.subscribe()
    }
}
