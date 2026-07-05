//! A `tracing_subscriber` layer that persists spans and events to the database.
//!
//! The layer captures tracing records and sends them through a bounded channel
//! to a background task that batch-inserts them via [`LogWriter`]. If the
//! channel is full, records are dropped and a synthetic warning event is
//! inserted to record how many were lost.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aperture_storage::{EventRecord, Level, LogWriter, SpanRecord};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tracing::span::{Attributes as SpanAttributes, Id as SpanId};
use tracing::{Event, Level as TracingLevel, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

use self::collector::FieldCollector;

mod collector;

/// Channel capacity. When full, records are dropped.
const CHANNEL_CAPACITY: usize = 4096;

/// How long the background writer waits for more records before flushing.
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Maximum records to batch before flushing.
const FLUSH_BATCH: usize = 128;

/// Name of the span wrapping every DB flush. Events emitted within this span
/// (turso trace events, etc.) are skipped in [`DbLogLayer::on_event`] to
/// prevent the feedback loop: flush writes to the DB, turso emits trace
/// events, those events would be captured and sent back to the flush task.
const FLUSH_SPAN_NAME: &str = "log_flush";

/// A record sent from the layer to the background writer.
enum Record {
    SpanStart(SpanStart),
    SpanEnd(SpanEnd),
    Event(EventMsg),
}

struct SpanStart {
    tracing_id: u64,
    parent_tracing_id: Option<u64>,
    name: String,
    level: Level,
    target: String,
    file: Option<String>,
    line: Option<u32>,
    started_at: Timestamp,
    fields: Option<String>,
}

struct SpanEnd {
    tracing_id: u64,
    ended_at: Timestamp,
}

struct EventMsg {
    span_tracing_id: Option<u64>,
    level: Level,
    target: String,
    message: Option<String>,
    timestamp: Timestamp,
    file: Option<String>,
    line: Option<u32>,
    fields: Option<String>,
}

/// A tracing layer that persists spans and events to the database.
///
/// Cheap to clone: all clones share one channel sender and drop counter.
#[derive(Clone)]
pub struct DbLogLayer {
    tx: mpsc::Sender<Record>,
    dropped: Arc<AtomicU64>,
    boot_id: Arc<Uuid>,
}

/// Handle to the background writer task. Keep it alive for as long as the
/// layer is active. Call [`shutdown`](Self::shutdown) for a clean flush
/// before the process exits.
///
/// If this handle is dropped without calling `shutdown`, the writer task is
/// aborted immediately and pending records may be lost.
pub struct WorkerHandle {
    join: Option<JoinHandle<()>>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl WorkerHandle {
    /// Signals the writer to drain remaining records, flush to the database,
    /// and exit. Waits for the writer to finish.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

impl DbLogLayer {
    /// Creates the layer and spawns the background writer task.
    ///
    /// Returns the layer and a [`WorkerHandle`] for clean shutdown. The handle
    /// should be kept alive for the lifetime of the application. Call
    /// [`WorkerHandle::shutdown`] before exiting to flush pending records.
    pub fn spawn(writer: LogWriter, boot_id: Uuid) -> (Self, WorkerHandle) {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let dropped = Arc::new(AtomicU64::new(0));
        let dropped_clone = dropped.clone();

        let join = tokio::spawn(writer_task(writer, rx, dropped_clone, shutdown_rx));

        let handle = WorkerHandle {
            join: Some(join),
            shutdown: Some(shutdown_tx),
        };

        (
            Self {
                tx,
                dropped,
                boot_id: Arc::new(boot_id),
            },
            handle,
        )
    }

    fn try_send(&self, record: Record) {
        if self.tx.try_send(record).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<S> Layer<S> for DbLogLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &SpanAttributes<'_>, id: &SpanId, _ctx: Context<'_, S>) {
        let meta = attrs.metadata();

        // Don't record the flush marker span. It exists only so on_event can
        // filter events emitted during the flush (feedback loop prevention).
        if meta.name() == FLUSH_SPAN_NAME {
            return;
        }

        let mut visitor = FieldCollector::new(&self.boot_id);
        attrs.record(&mut visitor);

        let tracing_id = id.into_u64();
        let parent_tracing_id = attrs.parent().map(|p| p.into_u64());

        self.try_send(Record::SpanStart(SpanStart {
            tracing_id,
            parent_tracing_id,
            name: meta.name().to_owned(),
            level: tracing_to_db_level(meta.level()),
            target: meta.target().to_owned(),
            file: meta.file().map(str::to_owned),
            line: meta.line(),
            started_at: Timestamp::now(),
            fields: visitor.into_json(),
        }));
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // Skip events emitted during a flush to prevent the feedback loop:
        // flush writes to the DB, turso emits trace events, those events
        // would be captured and sent back to the flush task.
        if is_within_flush_span(&ctx, event) {
            return;
        }

        let meta = event.metadata();
        let mut visitor = FieldCollector::new(&self.boot_id);
        event.record(&mut visitor);

        // For log-bridged events the metadata target is "log". The real
        // target lives in the `log.target` field. Use whichever is
        // available.
        let target = visitor
            .take_log_target()
            .unwrap_or_else(|| meta.target().to_owned());

        let span_tracing_id = ctx.event_span(event).map(|s| s.id().into_u64());

        let file = visitor
            .take_log_file()
            .or_else(|| meta.file().map(str::to_owned));
        let line = visitor.take_log_line().or_else(|| meta.line());

        self.try_send(Record::Event(EventMsg {
            span_tracing_id,
            level: tracing_to_db_level(meta.level()),
            target,
            message: visitor.take_message(),
            timestamp: Timestamp::now(),
            file,
            line,
            fields: visitor.into_json(),
        }));
    }

    fn on_close(&self, id: SpanId, _ctx: Context<'_, S>) {
        self.try_send(Record::SpanEnd(SpanEnd {
            tracing_id: id.into_u64(),
            ended_at: Timestamp::now(),
        }));
    }
}

/// Background writer task: drains the channel and batch-inserts records.
async fn writer_task(
    mut writer: LogWriter,
    mut rx: mpsc::Receiver<Record>,
    dropped: Arc<AtomicU64>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut span_ids: HashMap<u64, i64> = HashMap::new();
    let mut batch: Vec<Record> = Vec::with_capacity(FLUSH_BATCH);
    let mut interval = interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                while let Ok(record) = rx.try_recv() {
                    batch.push(record);
                }
                flush(&mut writer, &mut batch, &mut span_ids).await;
                flush_dropped(&mut writer, &dropped).await;
                break;
            }
            maybe_record = rx.recv() => {
                match maybe_record {
                    Some(record) => batch.push(record),
                    None => {
                        flush(&mut writer, &mut batch, &mut span_ids).await;
                        flush_dropped(&mut writer, &dropped).await;
                        break;
                    }
                }
                if batch.len() >= FLUSH_BATCH {
                    flush(&mut writer, &mut batch, &mut span_ids).await;
                    flush_dropped(&mut writer, &dropped).await;
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    flush(&mut writer, &mut batch, &mut span_ids).await;
                    flush_dropped(&mut writer, &dropped).await;
                }
            }
        }
    }
}

/// Checks whether `event` was emitted within the flush span (or any of its
/// descendants). Used to skip events generated by the DB engine during a
/// flush, preventing the feedback loop.
fn is_within_flush_span<S>(ctx: &Context<'_, S>, event: &Event<'_>) -> bool
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let Some(span) = ctx.event_span(event) else {
        return false;
    };
    span.scope()
        .any(|ancestor| ancestor.name() == FLUSH_SPAN_NAME)
}

/// Flushes a batch of records to the database using prepared statements.
/// Instrumented with [`FLUSH_SPAN_NAME`] so events emitted by the DB engine
/// during the flush are filtered by [`DbLogLayer::on_event`].
#[tracing::instrument(name = FLUSH_SPAN_NAME, level = "trace", skip_all)]
async fn flush(writer: &mut LogWriter, batch: &mut Vec<Record>, span_ids: &mut HashMap<u64, i64>) {
    for record in batch.drain(..) {
        match record {
            Record::SpanStart(s) => {
                let parent_db_id = s
                    .parent_tracing_id
                    .and_then(|pid| span_ids.get(&pid).copied());
                let result = writer
                    .insert_span(SpanRecord {
                        parent_id: parent_db_id,
                        name: &s.name,
                        level: s.level,
                        target: &s.target,
                        file: s.file.as_deref(),
                        line: s.line,
                        started_at: s.started_at,
                        fields: s.fields.as_deref(),
                    })
                    .await;
                if let Ok(db_id) = result {
                    span_ids.insert(s.tracing_id, db_id);
                }
            }
            Record::SpanEnd(s) => {
                if let Some(&db_id) = span_ids.get(&s.tracing_id) {
                    let _ = writer.close_span(db_id, s.ended_at).await;
                    span_ids.remove(&s.tracing_id);
                }
            }
            Record::Event(e) => {
                let span_db_id = e
                    .span_tracing_id
                    .and_then(|sid| span_ids.get(&sid).copied());
                let _ = writer
                    .insert_event(EventRecord {
                        span_id: span_db_id,
                        level: e.level,
                        target: &e.target,
                        message: e.message.as_deref(),
                        timestamp: e.timestamp,
                        file: e.file.as_deref(),
                        line: e.line,
                        fields: e.fields.as_deref(),
                    })
                    .await;
            }
        }
    }
}

/// Inserts a synthetic event for dropped records, if any.
#[tracing::instrument(name = FLUSH_SPAN_NAME, level = "trace", skip_all)]
async fn flush_dropped(writer: &mut LogWriter, dropped: &AtomicU64) {
    let count = dropped.swap(0, Ordering::Relaxed);
    if count > 0 {
        let _ = writer.record_dropped(count, Timestamp::now()).await;
    }
}

/// Maps a `tracing::Level` to the storage `Level`.
fn tracing_to_db_level(level: &TracingLevel) -> Level {
    match *level {
        TracingLevel::TRACE => Level::Trace,
        TracingLevel::DEBUG => Level::Debug,
        TracingLevel::INFO => Level::Info,
        TracingLevel::WARN => Level::Warn,
        TracingLevel::ERROR => Level::Error,
    }
}
