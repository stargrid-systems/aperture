//! Persists emitted events to storage in batches.
//!
//! The recorder is the only consumer that serializes payloads for storage,
//! and it does so inside flushes, off the emit path. One transaction per
//! flush. A failing flush is retried with capped backoff up to a deadline;
//! once the deadline passes, or shutdown races the retry, the batch is
//! dropped and the loss is logged with the id range.

use std::cmp;
use std::error::Error as StdError;
use std::time::{Duration, Instant};

use aperture_runtime::{BatchSink, Stop, Worker, run_batched};
use aperture_storage::{EventRepository, NewEvent, StorageError};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::bus::EventBus;
use crate::payload::EventEnvelope;

/// Flush after this many events, even if the interval has not elapsed.
const FLUSH_BATCH: usize = 64;

/// Flush at latest this long after the first pending event.
const FLUSH_INTERVAL: Duration = Duration::from_millis(200);

/// How long a failing flush is retried before the batch is dropped.
const RETRY_DEADLINE: Duration = Duration::from_secs(30);

/// First backoff between flush attempts. Doubles up to `MAX_BACKOFF`.
const RETRY_BACKOFF_START: Duration = Duration::from_millis(100);

/// Ceiling for the backoff between flush attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(5);

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
            stop.clone(),
            FLUSH_INTERVAL,
            FLUSH_BATCH,
            EventSink {
                repo: self.repo,
                stop,
                retry_deadline: RETRY_DEADLINE,
            },
        )
        .await;
    }
}

/// Feeds batches of events into the database. Used by
/// [`aperture_runtime::run_batched`].
struct EventSink {
    repo: EventRepository,
    stop: Stop,
    retry_deadline: Duration,
}

impl EventSink {
    /// Inserts `rows` in one transaction.
    async fn insert(&self, rows: &[NewEvent]) -> Result<(), StorageError> {
        let mut tx = self.repo.batch().await?;
        for row in rows {
            tx.insert(row).await?;
        }
        tx.commit().await
    }
}

impl BatchSink<EventEnvelope> for EventSink {
    async fn flush(&mut self, batch: &mut Vec<EventEnvelope>) {
        // Serialize up front: a payload that fails to serialize would
        // poison every retry, so it is dropped instead of retried.
        let mut rows = Vec::with_capacity(batch.len());
        for envelope in batch.drain(..) {
            match envelope.payload_json() {
                Ok(data) => rows.push(NewEvent {
                    id: envelope.id,
                    key: envelope.key().to_owned(),
                    data,
                    actor: envelope.actor,
                    timestamp: envelope.timestamp,
                }),
                Err(err) => {
                    tracing::warn!(
                        key = envelope.key(),
                        error = &err as &dyn StdError,
                        "failed to serialize event payload, dropping event"
                    );
                }
            }
        }
        if rows.is_empty() {
            return;
        }
        let first = rows[0].id;
        let last = rows[rows.len() - 1].id;

        let deadline = Instant::now() + self.retry_deadline;
        let mut backoff = RETRY_BACKOFF_START;
        loop {
            match self.insert(&rows).await {
                Ok(()) => return,
                Err(err) => {
                    if Instant::now() + backoff >= deadline || self.stop.is_cancelled() {
                        tracing::error!(
                            error = &err as &dyn StdError,
                            count = rows.len(),
                            first = %first,
                            last = %last,
                            "event batch flush failed, dropping batch"
                        );
                        return;
                    }
                    tracing::warn!(
                        error = &err as &dyn StdError,
                        "event batch flush failed, retrying"
                    );
                    tokio::select! {
                        biased;
                        () = self.stop.cancelled() => {}
                        () = sleep(backoff) => {
                            backoff = cmp::min(backoff * 2, MAX_BACKOFF);
                        }
                    }
                }
            }
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

    #[tokio::test]
    async fn failing_flush_drops_the_whole_batch() {
        let bus = EventBus::new();
        let storage = Storage::open(":memory:").await.unwrap();

        // The emitted id is already taken, so every insert of the batch fails
        // on the primary key and rolls the transaction back.
        let envelope = bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap();
        storage
            .events()
            .unwrap()
            .create(&NewEvent {
                id: envelope.id,
                key: Probe::KEY.to_owned(),
                data: serde_json::json!({ "n": 1 }),
                actor: ActorId::SYSTEM,
                timestamp: envelope.timestamp,
            })
            .await
            .unwrap();
        bus.emit(Probe { n: 2 }, ActorId::SYSTEM).await.unwrap();

        let rx = bus.take_recorder().expect("recorder available");
        let stop = aperture_runtime::Stop::new();
        let sink = run_batched(
            rx,
            stop.clone(),
            FLUSH_INTERVAL,
            FLUSH_BATCH,
            EventSink {
                repo: storage.events().unwrap(),
                stop: stop.clone(),
                retry_deadline: Duration::from_millis(50),
            },
        );
        stop.cancel();
        sink.await;

        let page = storage
            .events()
            .unwrap()
            .list(&EventFilter::default(), &ListQuery::default())
            .await
            .unwrap();
        // Only the pre-inserted row survives: the dropped batch took the
        // healthy event with it.
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, envelope.id);
    }
}
