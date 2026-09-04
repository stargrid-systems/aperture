//! Persists emitted events to storage in batches.
//!
//! The recorder is the only consumer that serializes payloads for storage,
//! and it does so inside flushes, off the emit path. One transaction per
//! chunk of up to [`FLUSH_BATCH`] events. A failing chunk is retried with
//! capped backoff within a retry window: 30 s while the recorder runs, then
//! a single 20 s budget shared by every flush call of the shutdown drain.
//! When the active window passes, the chunk is dropped and the loss is
//! logged with the id range. Earlier chunks stay committed and later chunks
//! are still attempted while budget remains.

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
/// Doubles as the transaction chunk size. Not aligned with the log worker's
/// batch bound; the two workers are tuned independently.
const FLUSH_BATCH: usize = 64;

/// Flush at latest this long after the first pending event.
const FLUSH_INTERVAL: Duration = Duration::from_millis(200);

/// How long a failing flush is retried while the recorder runs before
/// the batch is dropped.
const RETRY_DEADLINE: Duration = Duration::from_secs(30);

/// Total retry budget for a shutdown drain, shared by every flush call of
/// the sink so the whole queue drains within the supervisor's 30 s drain
/// timeout with headroom to spare.
const SHUTDOWN_DRAIN_BUDGET: Duration = Duration::from_secs(20);

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
                shutdown_budget: SHUTDOWN_DRAIN_BUDGET,
                shutdown_deadline: None,
            },
        )
        .await;
    }
}

/// Inserts serialized events into storage.
///
/// Implemented by [`EventRepository`] and by in-test stubs that control
/// when inserts fail.
trait InsertEvents: Sync + Send + 'static {
    fn insert(&self, rows: &[NewEvent]) -> impl Future<Output = Result<(), StorageError>> + Send;
}

impl InsertEvents for EventRepository {
    async fn insert(&self, rows: &[NewEvent]) -> Result<(), StorageError> {
        let mut tx = self.batch().await?;
        for row in rows {
            tx.insert(row).await?;
        }
        tx.commit().await
    }
}

/// Feeds batches of events into the database. Used by
/// [`aperture_runtime::run_batched`].
struct EventSink<R> {
    repo: R,
    stop: Stop,
    /// How long a failing flush is retried while the recorder runs.
    retry_deadline: Duration,
    /// Total retry budget shared by all chunks of a shutdown drain.
    shutdown_budget: Duration,
    /// Drop deadline of the shutdown budget, set on the first flush call
    /// that observes stop, so no later flush call regrants the budget.
    shutdown_deadline: Option<Instant>,
}

/// Drop deadline for a retry window, or `None` when the clock overflows.
fn deadline_after(window: Duration) -> Option<Instant> {
    Instant::now().checked_add(window)
}

impl<R: InsertEvents> BatchSink<EventEnvelope> for EventSink<R> {
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
                    tracing::error!(
                        id = %envelope.id,
                        key = envelope.key(),
                        error = &err as &dyn StdError,
                        "failed to serialize event payload, dropping event"
                    );
                }
            }
        }

        // The shutdown path hands the whole queue over at once (up to the
        // recorder channel's capacity plus one in-flight batch), so flush
        // in chunks of FLUSH_BATCH, one transaction per chunk: a failing
        // chunk loses only itself, and later chunks are still attempted.
        // Every flush call of the drain draws on the same budget stored on
        // the sink, so the whole queue drains within the supervisor's
        // drain timeout.
        while !rows.is_empty() {
            if self
                .shutdown_deadline
                .is_some_and(|at| Instant::now() >= at)
            {
                tracing::error!(
                    count = rows.len(),
                    first = %rows[0].id,
                    last = %rows[rows.len() - 1].id,
                    "event shutdown drain budget exhausted, dropping remaining events"
                );
                break;
            }
            let take = FLUSH_BATCH.min(rows.len());
            let chunk: Vec<NewEvent> = rows.drain(..take).collect();
            self.flush_chunk(chunk).await;
        }
    }
}

impl<R: InsertEvents> EventSink<R> {
    /// Drop deadline for the active retry window: the shared shutdown
    /// budget once stop is cancelled, a fresh running window per chunk
    /// before that.
    fn deadline(&mut self, stopped: bool) -> Option<Instant> {
        if !stopped {
            return deadline_after(self.retry_deadline);
        }
        if self.shutdown_deadline.is_none() {
            // A `None` deadline (clock overflow) never expires, matching
            // the running window above.
            self.shutdown_deadline = deadline_after(self.shutdown_budget);
        }
        self.shutdown_deadline
    }

    /// Flushes one chunk in a single transaction. On failure the chunk is
    /// retried with capped backoff within the active window and dropped with
    /// its id range when the window passes.
    async fn flush_chunk(&mut self, rows: Vec<NewEvent>) {
        if rows.is_empty() {
            return;
        }
        let first = rows[0].id;
        let last = rows[rows.len() - 1].id;

        // Once stop is cancelled every chunk drains against the one shared
        // shutdown budget, so a slow chunk cannot starve the rest.
        let mut stopped = self.stop.is_cancelled();
        let mut deadline = self.deadline(stopped);
        let mut backoff = RETRY_BACKOFF_START;
        loop {
            match self.repo.insert(&rows).await {
                Ok(()) => return,
                Err(err) => {
                    if !stopped && self.stop.is_cancelled() {
                        stopped = true;
                        deadline = self.deadline(true);
                    }
                    if deadline.is_none_or(|at| Instant::now() >= at) {
                        tracing::error!(
                            error = &err as &dyn StdError,
                            count = rows.len(),
                            first = %first,
                            last = %last,
                            "event chunk flush failed, dropping chunk"
                        );
                        return;
                    }
                    tracing::warn!(
                        error = &err as &dyn StdError,
                        "event chunk flush failed, retrying"
                    );
                    if stopped {
                        // The token is already cancelled: selecting on it
                        // would spin the loop, so pace with a plain sleep
                        // clamped to the remaining budget.
                        let pause = deadline.map_or(backoff, |at| {
                            cmp::min(backoff, at.saturating_duration_since(Instant::now()))
                        });
                        sleep(pause).await;
                        backoff = cmp::min(backoff * 2, MAX_BACKOFF);
                    } else {
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
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use aperture_storage::{ActorId, Event, EventFilter, ListQuery, Storage};
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

    /// Fails the first `failures` inserts, then delegates to the wrapped
    /// repository. Records the time of every insert attempt.
    struct FlakyRepo {
        repo: EventRepository,
        failures: Mutex<usize>,
        attempts: Arc<Mutex<Vec<Instant>>>,
    }

    impl InsertEvents for FlakyRepo {
        async fn insert(&self, rows: &[NewEvent]) -> Result<(), StorageError> {
            self.attempts
                .lock()
                .expect("attempts lock poisoned")
                .push(Instant::now());
            let forced_failure = {
                let mut failures = self.failures.lock().expect("failures lock poisoned");
                if *failures > 0 {
                    *failures -= 1;
                    true
                } else {
                    false
                }
            };
            if forced_failure {
                return Err(StorageError::InvalidCursor(String::from(
                    "synthetic insert failure",
                )));
            }
            self.repo.insert(rows).await
        }
    }

    /// Runs the recorder to completion (drain + flush) and lists what got
    /// persisted, newest first.
    async fn recorded(bus: &EventBus, storage: &Storage) -> Vec<Event> {
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
    }

    #[tokio::test]
    async fn emits_are_persisted_after_drain() {
        let bus = EventBus::new();
        let storage = Storage::open(":memory:").await.unwrap();

        bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap();
        bus.emit(Probe { n: 2 }, ActorId::SYSTEM).await.unwrap();

        // Newest first, but same-microsecond emits tie on the random uuid
        // suffix, so only membership and non-strict order hold.
        let items = recorded(&bus, &storage).await;
        let mut ns: Vec<u64> = items
            .iter()
            .map(|event| event.data["n"].as_u64().unwrap())
            .collect();
        ns.sort_unstable();
        assert_eq!(ns, [1, 2]);
        let mut listed: Vec<i64> = items
            .iter()
            .map(|event| event.timestamp.as_microsecond())
            .collect();
        let mut sorted = listed.clone();
        sorted.sort_unstable();
        listed.reverse();
        assert_eq!(listed, sorted, "must be non-strictly ordered newest first");
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
        // The flush runs with the token already cancelled, so the
        // shrunken shutdown window governs the drop.
        let sink = run_batched(
            rx,
            stop.clone(),
            FLUSH_INTERVAL,
            FLUSH_BATCH,
            EventSink {
                repo: storage.events().unwrap(),
                stop: stop.clone(),
                retry_deadline: Duration::from_millis(50),
                shutdown_budget: Duration::from_millis(50),
                shutdown_deadline: None,
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

    #[tokio::test]
    async fn failing_inserts_are_retried_until_one_succeeds() {
        let bus = EventBus::new();
        let storage = Storage::open(":memory:").await.unwrap();

        bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap();
        let mut rx = bus.take_recorder().expect("recorder available");
        let mut batch = Vec::new();
        while let Ok(envelope) = rx.try_recv() {
            batch.push(envelope);
        }

        let attempts = Arc::new(Mutex::new(Vec::new()));
        let mut sink = EventSink {
            repo: FlakyRepo {
                repo: storage.events().unwrap(),
                failures: Mutex::new(2),
                attempts: attempts.clone(),
            },
            stop: aperture_runtime::Stop::new(),
            retry_deadline: RETRY_DEADLINE,
            shutdown_budget: SHUTDOWN_DRAIN_BUDGET,
            shutdown_deadline: None,
        };
        BatchSink::flush(&mut sink, &mut batch).await;

        let attempts = attempts.lock().expect("attempts lock poisoned").clone();
        assert_eq!(attempts.len(), 3);
        // Timer sleeps never fire early, so the attempts are paced by the
        // doubling backoff.
        assert!(attempts[1] - attempts[0] >= RETRY_BACKOFF_START);
        assert!(attempts[2] - attempts[1] >= RETRY_BACKOFF_START * 2);

        let page = storage
            .events()
            .unwrap()
            .list(&EventFilter::default(), &ListQuery::default())
            .await
            .unwrap();
        // The third attempt persisted the batch.
        assert_eq!(page.items.len(), 1);
    }

    #[tokio::test]
    async fn shutdown_flush_retries_before_dropping_the_batch() {
        let bus = EventBus::new();
        let storage = Storage::open(":memory:").await.unwrap();

        // The event sits in the recorder queue when stop fires, so the
        // drain flush runs with the token already cancelled.
        bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap();
        let rx = bus.take_recorder().expect("recorder available");
        let stop = aperture_runtime::Stop::new();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let sink = run_batched(
            rx,
            stop.clone(),
            FLUSH_INTERVAL,
            FLUSH_BATCH,
            EventSink {
                repo: FlakyRepo {
                    repo: storage.events().unwrap(),
                    failures: Mutex::new(usize::MAX),
                    attempts: attempts.clone(),
                },
                stop: stop.clone(),
                retry_deadline: RETRY_DEADLINE,
                shutdown_budget: Duration::from_millis(500),
                shutdown_deadline: None,
            },
        );
        stop.cancel();
        sink.await;

        let attempts = attempts.lock().expect("attempts lock poisoned").clone();
        // Without the shutdown window the cancelled token would drop the
        // batch after a single attempt.
        assert!(attempts.len() > 1);

        let page = storage
            .events()
            .unwrap()
            .list(&EventFilter::default(), &ListQuery::default())
            .await
            .unwrap();
        // Every attempt failed, so the batch was dropped and logged.
        assert!(page.items.is_empty());
    }

    /// Fails every insert of a full chunk, delegates smaller ones. Records
    /// the time of every insert attempt.
    struct FullChunkFails {
        repo: EventRepository,
        attempts: Arc<Mutex<Vec<Instant>>>,
    }

    impl InsertEvents for FullChunkFails {
        async fn insert(&self, rows: &[NewEvent]) -> Result<(), StorageError> {
            self.attempts
                .lock()
                .expect("attempts lock poisoned")
                .push(Instant::now());
            if rows.len() >= FLUSH_BATCH {
                return Err(StorageError::InvalidCursor(String::from(
                    "synthetic insert failure",
                )));
            }
            self.repo.insert(rows).await
        }
    }

    /// The shutdown drain's retry budget is shared by all chunks: once an
    /// earlier chunk exhausts it, the remaining chunks are dropped instead
    /// of getting a fresh window that would outlast the supervisor's drain
    /// timeout.
    #[tokio::test]
    async fn shutdown_drain_budget_is_shared_across_chunks() {
        let bus = EventBus::new();
        let storage = Storage::open(":memory:").await.unwrap();

        // Two chunks: the first fails on every attempt, the second would
        // succeed immediately.
        let count = u32::try_from(FLUSH_BATCH).unwrap() + 1;
        for n in 0..count {
            bus.emit(Probe { n }, ActorId::SYSTEM).await.unwrap();
        }
        let rx = bus.take_recorder().expect("recorder available");
        let stop = aperture_runtime::Stop::new();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let sink = run_batched(
            rx,
            stop.clone(),
            FLUSH_INTERVAL,
            FLUSH_BATCH,
            EventSink {
                repo: FullChunkFails {
                    repo: storage.events().unwrap(),
                    attempts: attempts.clone(),
                },
                stop: stop.clone(),
                retry_deadline: RETRY_DEADLINE,
                shutdown_budget: Duration::from_millis(200),
                shutdown_deadline: None,
            },
        );
        stop.cancel();
        sink.await;

        let attempts = attempts.lock().expect("attempts lock poisoned").clone();
        // The failing first chunk was retried until the shared budget ran
        // out, not dropped after a single attempt.
        assert!(attempts.len() >= 2, "the failing chunk must be retried");

        let page = storage
            .events()
            .unwrap()
            .list(&EventFilter::default(), &ListQuery::default())
            .await
            .unwrap();
        // The healthy second chunk got no fresh window: it was dropped with
        // the exhausted budget.
        assert!(page.items.is_empty());
    }

    /// The shutdown budget lives on the sink, not on a single flush call:
    /// once one flush call exhausted it, a later flush call must not get a
    /// fresh window. Otherwise repeated flushes (batch-full, interval, and
    /// the stop drain) could stack budgets past the supervisor's drain
    /// timeout.
    #[tokio::test]
    async fn shutdown_budget_is_shared_across_flush_calls() {
        let bus = EventBus::new();
        let storage = Storage::open(":memory:").await.unwrap();

        bus.emit(Probe { n: 1 }, ActorId::SYSTEM).await.unwrap();
        bus.emit(Probe { n: 2 }, ActorId::SYSTEM).await.unwrap();
        let mut rx = bus.take_recorder().expect("recorder available");

        let attempts = Arc::new(Mutex::new(Vec::new()));
        let stop = aperture_runtime::Stop::new();
        let mut sink = EventSink {
            repo: FlakyRepo {
                repo: storage.events().unwrap(),
                failures: Mutex::new(usize::MAX),
                attempts: attempts.clone(),
            },
            stop: stop.clone(),
            retry_deadline: RETRY_DEADLINE,
            shutdown_budget: Duration::from_millis(200),
            shutdown_deadline: None,
        };
        stop.cancel();

        let mut first = vec![rx.try_recv().expect("first event")];
        BatchSink::flush(&mut sink, &mut first).await;
        let after_first = attempts.lock().expect("attempts lock poisoned").len();
        assert!(after_first >= 2, "the first flush must be retried");

        let mut second = vec![rx.try_recv().expect("second event")];
        BatchSink::flush(&mut sink, &mut second).await;
        assert_eq!(
            attempts.lock().expect("attempts lock poisoned").len(),
            after_first,
            "the second flush must not get a fresh budget"
        );
    }
}
