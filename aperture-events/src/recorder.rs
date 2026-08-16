//! Persists emitted events to storage in batches.
//!
//! The recorder is the only consumer that serializes payloads for storage,
//! and it does so inside flushes, off the emit path. One transaction per
//! flush; a failed flush loses its batch and is logged, mirroring the log
//! worker.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_runtime::{BatchSink, Stop, Worker, run_batched};
use aperture_storage::{EventRepository, NewEvent};
use tokio::sync::mpsc;

use crate::bus::EventBus;
use crate::payload::EventEnvelope;

/// Flush after this many events, even if the interval has not elapsed.
const FLUSH_BATCH: usize = 64;

/// Flush at latest this long after the first pending event.
const FLUSH_INTERVAL: Duration = Duration::from_millis(200);

/// Drains the bus's recorder channel and batch-inserts the events.
///
/// Produced by [`EventRecorder::connect`]. Drive it via a
/// [`aperture_runtime::Supervisor`] so it flushes pending events during
/// shutdown.
pub struct EventRecorder {
    rx: mpsc::Receiver<EventEnvelope>,
    repo: EventRepository,
}

impl EventRecorder {
    /// Takes the recorder channel from `bus` and returns the worker that
    /// persists batches into `repo`.
    ///
    /// Returns `None` if the recorder was already connected: one bus has
    /// exactly one recorder.
    pub fn connect(bus: &EventBus, repo: EventRepository) -> Option<Self> {
        bus.take_recorder().map(|rx| Self { rx, repo })
    }
}

impl Worker for EventRecorder {
    async fn run(self, stop: Stop) {
        run_batched(
            self.rx,
            stop,
            FLUSH_INTERVAL,
            FLUSH_BATCH,
            EventSink { repo: self.repo },
        )
        .await;
    }
}

/// Feeds batches of events into the database. Used by
/// [`aperture_runtime::run_batched`].
struct EventSink {
    repo: EventRepository,
}

impl BatchSink<EventEnvelope> for EventSink {
    async fn flush(&mut self, batch: &mut Vec<EventEnvelope>) {
        let mut tx = match self.repo.batch().await {
            Ok(tx) => tx,
            Err(err) => {
                tracing::error!(error = &err as &dyn StdError, "failed to open event batch");
                return;
            }
        };
        // Any insert error poisons the transaction. Fail fast so we do not
        // waste time on doomed follow-up inserts. The drained batch is
        // lost, matching the log worker's failure semantics.
        for envelope in batch.drain(..) {
            let data = match envelope.payload_json() {
                Ok(data) => data,
                Err(err) => {
                    tracing::warn!(
                        key = envelope.key(),
                        error = &err as &dyn StdError,
                        "failed to serialize event payload, skipping"
                    );
                    continue;
                }
            };
            let new = NewEvent {
                id: envelope.id,
                key: envelope.key().to_owned(),
                data,
                actor: envelope.actor,
                timestamp: envelope.timestamp,
            };
            if let Err(err) = tx.insert(&new).await {
                tracing::warn!(
                    error = &err as &dyn StdError,
                    "event batch insert failed, rolling back"
                );
                return;
            }
        }
        if let Err(err) = tx.commit().await {
            tracing::error!(
                error = &err as &dyn StdError,
                "failed to commit event batch"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use aperture_storage::{ActorId, EventFilter, ListQuery, Storage};
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    use super::*;
    use crate::definition::EventDefinition;

    #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
    struct Probe {
        n: u32,
    }

    impl EventDefinition for Probe {
        const KEY: &'static str = "test.probe";
    }

    /// Runs the recorder to completion (drain + flush) and lists what got
    /// persisted, newest first.
    async fn recorded(bus: &EventBus, storage: &Storage) -> Vec<u64> {
        let recorder = EventRecorder::connect(bus, storage.events().unwrap()).expect("recorder");
        let stop = aperture_runtime::Stop::new();
        let worker = tokio::spawn(recorder.run(stop.clone()));
        stop.cancel();
        worker.await.unwrap();
        let page = storage
            .events()
            .unwrap()
            .list(&EventFilter::default(), &ListQuery::default())
            .await
            .unwrap();
        page.items
            .iter()
            .map(|event| event.data["n"].as_u64().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn emits_are_persisted_after_drain() {
        let bus = EventBus::new();
        let storage = Storage::open(":memory:").await.unwrap();

        bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap();
        bus.emit(Probe { n: 2 }, ActorId::SYSTEM).await.unwrap();

        // Newest first: the recorder keeps emit order and the list is DESC.
        let ns = recorded(&bus, &storage).await;
        assert_eq!(ns, vec![2, 1]);
    }
}
