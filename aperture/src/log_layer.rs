//! A `tracing_subscriber` layer that persists spans and events to the database.
//!
//! The layer captures tracing records and sends them through a bounded channel
//! to a background task that batch-inserts them via [`LogRepository`]. If the
//! channel is full, records are dropped and a synthetic warning event is
//! inserted to record how many were lost.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aperture_artifacts::{Level, LogRepository};
use jiff::Timestamp;
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes as SpanAttributes, Id as SpanId};
use tracing::{Event, Level as TracingLevel, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;
use tokio::time::{MissedTickBehavior, interval};

/// A record sent from the layer to the background writer.
enum Record {
    SpanStart(SpanStart),
    SpanEnd(SpanEnd),
    Event(EventRecord),
}

struct SpanStart {
    tracing_id: u64,
    parent_tracing_id: Option<u64>,
    name: String,
    level: Level,
    target: String,
    file: Option<String>,
    line: Option<i64>,
    started_at: Timestamp,
    fields: Option<String>,
}

struct SpanEnd {
    tracing_id: u64,
    ended_at: Timestamp,
}

struct EventRecord {
    span_tracing_id: Option<u64>,
    level: Level,
    target: String,
    message: Option<String>,
    timestamp: Timestamp,
    file: Option<String>,
    line: Option<i64>,
    fields: Option<String>,
}

/// Channel capacity. When full, records are dropped.
const CHANNEL_CAPACITY: usize = 4096;

/// How long the background writer waits for more records before flushing.
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Maximum records to batch before flushing.
const FLUSH_BATCH: usize = 128;

/// A tracing layer that persists spans and events to the database.
///
/// Cheap to clone: all clones share one channel sender and drop counter.
#[derive(Clone)]
pub struct DbLogLayer {
    tx: mpsc::Sender<Record>,
    dropped: Arc<AtomicU64>,
}

impl DbLogLayer {
    /// Creates the layer and spawns the background writer task.
    ///
    /// Returns the layer. The background task runs for the lifetime of the
    /// process and should not be cancelled.
    pub fn spawn(repo: LogRepository) -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let dropped_clone = dropped.clone();

        tokio::spawn(writer_task(repo, rx, dropped_clone));

        Self { tx, dropped }
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
    fn on_new_span(
        &self,
        attrs: &SpanAttributes<'_>,
        id: &SpanId,
        _ctx: Context<'_, S>,
    ) {
        let meta = attrs.metadata();
        let mut visitor = FieldCollector::new();
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
            line: meta.line().map(|l| l as i64),
            started_at: now(),
            fields: visitor.into_json(),
        }));
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = FieldCollector::new();
        event.record(&mut visitor);

        let span_tracing_id = ctx.event_span(event).map(|s| s.id().into_u64());

        self.try_send(Record::Event(EventRecord {
            span_tracing_id,
            level: tracing_to_db_level(meta.level()),
            target: meta.target().to_owned(),
            message: visitor.take_message(),
            timestamp: now(),
            file: meta.file().map(str::to_owned),
            line: meta.line().map(|l| l as i64),
            fields: visitor.into_json(),
        }));
    }

    fn on_close(&self, id: SpanId, _ctx: Context<'_, S>) {
        self.try_send(Record::SpanEnd(SpanEnd {
            tracing_id: id.into_u64(),
            ended_at: now(),
        }));
    }
}

/// Background writer task: drains the channel and batch-inserts records.
async fn writer_task(repo: LogRepository, mut rx: mpsc::Receiver<Record>, dropped: Arc<AtomicU64>) {
    let mut span_ids: HashMap<u64, i64> = HashMap::new();
    let mut batch: Vec<Record> = Vec::with_capacity(FLUSH_BATCH);
    let mut interval = interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            maybe_record = rx.recv() => {
                match maybe_record {
                    Some(record) => batch.push(record),
                    None => {
                        flush(&repo, &mut batch, &mut span_ids).await;
                        flush_dropped(&repo, &dropped).await;
                        break;
                    }
                }
                if batch.len() >= FLUSH_BATCH {
                    flush(&repo, &mut batch, &mut span_ids).await;
                    flush_dropped(&repo, &dropped).await;
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    flush(&repo, &mut batch, &mut span_ids).await;
                    flush_dropped(&repo, &dropped).await;
                }
            }
        }
    }
}

/// Flushes a batch of records to the database.
async fn flush(repo: &LogRepository, batch: &mut Vec<Record>, span_ids: &mut HashMap<u64, i64>) {
    for record in batch.drain(..) {
        match record {
            Record::SpanStart(s) => {
                let parent_db_id = s.parent_tracing_id.and_then(|pid| span_ids.get(&pid).copied());
                let result = repo
                    .insert_span(
                        parent_db_id,
                        &s.name,
                        s.level,
                        &s.target,
                        s.file.as_deref(),
                        s.line,
                        s.started_at,
                        s.fields.as_deref(),
                    )
                    .await;
                if let Ok(db_id) = result {
                    span_ids.insert(s.tracing_id, db_id);
                }
            }
            Record::SpanEnd(s) => {
                if let Some(&db_id) = span_ids.get(&s.tracing_id) {
                    let _ = repo.close_span(db_id, s.ended_at).await;
                    span_ids.remove(&s.tracing_id);
                }
            }
            Record::Event(e) => {
                let span_db_id = e.span_tracing_id.and_then(|sid| span_ids.get(&sid).copied());
                let _ = repo
                    .insert_event(
                        span_db_id,
                        e.level,
                        &e.target,
                        e.message.as_deref(),
                        e.timestamp,
                        e.file.as_deref(),
                        e.line,
                        e.fields.as_deref(),
                    )
                    .await;
            }
        }
    }
}

/// Inserts a synthetic event for dropped records, if any.
async fn flush_dropped(repo: &LogRepository, dropped: &AtomicU64) {
    let count = dropped.swap(0, Ordering::Relaxed);
    if count > 0 {
        let _ = repo.record_dropped(count, now()).await;
    }
}

/// Field visitor that collects all fields into a JSON object and extracts the
/// "message" field specially.
struct FieldCollector {
    fields: Map<String, Value>,
    message: Option<String>,
}

impl FieldCollector {
    fn new() -> Self {
        Self {
            fields: Map::new(),
            message: None,
        }
    }

    fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    fn into_json(self) -> Option<String> {
        if self.fields.is_empty() {
            None
        } else {
            serde_json::to_string(&Value::Object(self.fields)).ok()
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        let value_str = format!("{value:?}");
        self.store(field, Value::String(value_str));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.store(field, Value::String(value.to_owned()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.store(field, Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.store(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.store(field, Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.store(field, serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null));
    }
}

impl FieldCollector {
    fn store(&mut self, field: &Field, value: Value) {
        let name = field.name();
        if name == "message"
            && let Value::String(s) = &value
        {
            self.message = Some(s.clone());
        }
        self.fields.insert(name.to_owned(), value);
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

/// Returns the current timestamp.
fn now() -> Timestamp {
    Timestamp::now()
}
