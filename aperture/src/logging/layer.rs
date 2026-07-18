//! A `tracing_subscriber` layer that persists spans and events to the database.
//!
//! The layer captures tracing records and sends them through a bounded channel
//! to a background writer that batch-inserts them via [`LogRepository`] in a
//! single transaction per flush. If the channel is full, records are dropped
//! and a synthetic warning event is inserted to record how many were lost.

use std::error::Error as StdError;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aperture_storage::{EventRecord, Level, LogRepository, SpanRecord};
use jiff::Timestamp;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};
use tracing::span::{Attributes as SpanAttributes, Id as SpanId, Record as TracingRecord};
use tracing::{Event, Level as TracingLevel, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

use self::collector::FieldCollector;
use crate::runtime::Worker;

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
    SpanFields(SpanFields),
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
    fields: serde_json::Map<String, serde_json::Value>,
}

struct SpanEnd {
    tracing_id: u64,
    ended_at: Timestamp,
}

struct SpanFields {
    tracing_id: u64,
    fields: serde_json::Map<String, serde_json::Value>,
}

struct EventMsg {
    span_tracing_id: Option<u64>,
    level: Level,
    target: String,
    message: Option<String>,
    timestamp: Timestamp,
    file: Option<String>,
    line: Option<u32>,
    fields: serde_json::Map<String, serde_json::Value>,
}

/// A tracing layer that persists spans and events to the database.
///
/// Cheap to clone: all clones share one channel sender and drop counter.
#[derive(Clone)]
pub struct DbLogLayer {
    tx: mpsc::Sender<Record>,
    dropped: Arc<AtomicU64>,
}

/// Drains the layer's channel and batch-inserts records into the database.
/// Produced by [`DbLogLayer::new`]; drive it via a [`Supervisor`] so it shuts
/// down alongside the rest of the gateway.
///
/// [`Supervisor`]: crate::runtime::Supervisor
pub struct LogWorker {
    rx: mpsc::Receiver<Record>,
    repo: LogRepository,
    dropped: Arc<AtomicU64>,
    boot_id: Uuid,
}

impl Worker for LogWorker {
    async fn run(mut self, stop: impl Future<Output = ()> + Send + 'static) {
        let mut batch: Vec<Record> = Vec::with_capacity(FLUSH_BATCH);
        let mut interval = interval(FLUSH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut stop = Box::pin(stop);
        loop {
            tokio::select! {
                biased;
                () = &mut stop => {
                    while let Ok(record) = self.rx.try_recv() {
                        batch.push(record);
                    }
                    flush(&self.repo, &mut batch, &self.dropped, self.boot_id).await;
                    close_remaining_spans(&self.repo).await;
                    break;
                }
                maybe_record = self.rx.recv() => {
                    match maybe_record {
                        Some(record) => batch.push(record),
                        None => {
                            flush(&self.repo, &mut batch, &self.dropped, self.boot_id).await;
                            close_remaining_spans(&self.repo).await;
                            break;
                        }
                    }
                    if batch.len() >= FLUSH_BATCH {
                        flush(&self.repo, &mut batch, &self.dropped, self.boot_id).await;
                    }
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        flush(&self.repo, &mut batch, &self.dropped, self.boot_id).await;
                    }
                }
            }
        }
    }
}

impl DbLogLayer {
    fn channel() -> (Self, mpsc::Receiver<Record>) {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        (Self { tx, dropped }, rx)
    }

    pub fn new(repo: LogRepository, boot_id: Uuid) -> (Self, LogWorker) {
        let (layer, rx) = Self::channel();
        let worker = LogWorker {
            rx,
            repo,
            dropped: Arc::clone(&layer.dropped),
            boot_id,
        };
        (layer, worker)
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
    fn on_new_span(&self, attrs: &SpanAttributes<'_>, id: &SpanId, ctx: Context<'_, S>) {
        let meta = attrs.metadata();

        // Don't record the flush marker span or any span created within it.
        // The flush span exists only so on_event/on_new_span can filter events
        // and spans emitted during the flush (feedback loop prevention).
        if meta.name() == FLUSH_SPAN_NAME || is_span_within_flush(&ctx, attrs) {
            return;
        }

        let mut visitor = FieldCollector::new(meta.fields());
        attrs.record(&mut visitor);

        let tracing_id = id.into_u64();
        let parent_tracing_id = attrs
            .parent()
            .map(|p| p.into_u64())
            .or_else(|| ctx.lookup_current().map(|s| s.id().into_u64()));

        self.try_send(Record::SpanStart(SpanStart {
            tracing_id,
            parent_tracing_id,
            name: meta.name().to_owned(),
            level: tracing_to_db_level(meta.level()),
            target: meta.target().to_owned(),
            file: meta.file().map(str::to_owned),
            line: meta.line(),
            started_at: Timestamp::now(),
            fields: visitor.into_fields(),
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
        let mut visitor = FieldCollector::new(meta.fields());
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
            fields: visitor.into_fields(),
        }));
    }

    fn on_close(&self, id: SpanId, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id)
            && span
                .scope()
                .any(|ancestor| ancestor.name() == FLUSH_SPAN_NAME)
        {
            return;
        }
        self.try_send(Record::SpanEnd(SpanEnd {
            tracing_id: id.into_u64(),
            ended_at: Timestamp::now(),
        }));
    }

    fn on_record(&self, id: &SpanId, values: &TracingRecord<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && span
                .scope()
                .any(|ancestor| ancestor.name() == FLUSH_SPAN_NAME)
        {
            return;
        }
        let mut visitor = FieldCollector::additional();
        values.record(&mut visitor);
        let fields = visitor.into_fields();
        self.try_send(Record::SpanFields(SpanFields {
            tracing_id: id.into_u64(),
            fields,
        }));
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

/// Checks whether a new span is being created within the flush span. Used to
/// skip spans generated during a flush (e.g. instrumented storage methods).
fn is_span_within_flush<S>(ctx: &Context<'_, S>, attrs: &SpanAttributes<'_>) -> bool
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    if let Some(parent) = attrs.parent().and_then(|id| ctx.span(id)) {
        return parent
            .scope()
            .any(|ancestor| ancestor.name() == FLUSH_SPAN_NAME);
    }
    ctx.lookup_current().is_some_and(|current| {
        current
            .scope()
            .any(|ancestor| ancestor.name() == FLUSH_SPAN_NAME)
    })
}

/// Flushes a batch of records to the database in a single transaction.
///
/// Instrumented with [`FLUSH_SPAN_NAME`] so events emitted by the DB engine
/// during the flush are filtered by [`DbLogLayer::on_event`].
#[tracing::instrument(name = FLUSH_SPAN_NAME, level = "trace", skip_all)]
async fn flush(repo: &LogRepository, batch: &mut Vec<Record>, dropped: &AtomicU64, boot_id: Uuid) {
    let mut tx = match repo.batch().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(error = &err as &dyn StdError, "failed to open log batch");
            return;
        }
    };
    for record in batch.drain(..) {
        match record {
            Record::SpanStart(s) => {
                let _ = tx
                    .insert_span(SpanRecord {
                        tracing_id: s.tracing_id,
                        parent_tracing_id: s.parent_tracing_id,
                        boot_id,
                        name: &s.name,
                        level: s.level,
                        target: &s.target,
                        file: s.file.as_deref(),
                        line: s.line,
                        started_at: s.started_at,
                        fields: &s.fields,
                    })
                    .await;
            }
            Record::SpanFields(f) => {
                let _ = tx
                    .update_span_fields(f.tracing_id, boot_id, &f.fields)
                    .await;
            }
            Record::SpanEnd(s) => {
                let _ = tx.close_span(s.tracing_id, boot_id, s.ended_at).await;
            }
            Record::Event(e) => {
                let _ = tx
                    .insert_event(EventRecord {
                        span_tracing_id: e.span_tracing_id,
                        level: e.level,
                        target: &e.target,
                        message: e.message.as_deref(),
                        timestamp: e.timestamp,
                        file: e.file.as_deref(),
                        line: e.line,
                        boot_id,
                        fields: &e.fields,
                    })
                    .await;
            }
        }
    }
    let count = dropped.load(Ordering::Relaxed);
    if count > 0 {
        let _ = tx.record_dropped(count, Timestamp::now(), boot_id).await;
    }
    match tx.commit().await {
        Ok(()) => {
            if count > 0 {
                dropped.fetch_sub(count, Ordering::Relaxed);
            }
        }
        Err(err) => {
            tracing::error!(error = &err as &dyn StdError, "failed to commit log batch");
        }
    }
}

/// Closes every span left open after the writer task exits. Spans may remain
/// open if the process was interrupted or if a span was never explicitly
/// closed.
async fn close_remaining_spans(repo: &LogRepository) {
    if let Err(err) = repo.close_open_spans(Timestamp::now()).await {
        tracing::warn!(
            error = &err as &dyn StdError,
            "failed to close open spans on shutdown"
        );
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

#[cfg(test)]
mod tests {
    use tracing::Dispatch;
    use tracing_subscriber::prelude::*;

    use super::*;

    /// Events and spans emitted within a `"log_flush"` span must be filtered
    /// out. This is the feedback loop prevention: the flush writes to the DB,
    /// the DB engine emits events, those events would fill the channel.
    #[test]
    fn flush_span_filters_events_and_child_spans() {
        let (layer, mut rx) = DbLogLayer::channel();
        let subscriber = tracing_subscriber::registry().with(layer);
        let dispatcher = Dispatch::new(subscriber);

        let _guard = dispatcher.clone().set_default();

        // Event outside the flush span -> captured.
        tracing::info!("outside");

        // Enter the flush span (same name as FLUSH_SPAN_NAME).
        let flush = tracing::info_span!("log_flush");
        let _flush_guard = flush.enter();

        // Child span within flush -> filtered.
        let child = tracing::info_span!("storage_method");
        let _child_guard = child.enter();

        // Event within flush and child span -> filtered.
        tracing::warn!("inside flush");

        drop(_child_guard);
        drop(_flush_guard);

        // Collect all records.
        let mut events = Vec::new();
        let mut span_starts = 0;
        let mut span_ends = 0;
        while let Ok(record) = rx.try_recv() {
            match record {
                Record::Event(e) => events.push(e),
                Record::SpanStart(_) => span_starts += 1,
                Record::SpanEnd(_) => span_ends += 1,
                Record::SpanFields(_) => {}
            }
        }

        assert_eq!(events.len(), 1, "only the outside event should be captured");
        assert_eq!(
            events[0].message.as_deref(),
            Some("outside"),
            "captured event should be 'outside'"
        );
        assert_eq!(
            span_starts, 0,
            "no spans should be recorded from within flush"
        );
        assert_eq!(
            span_ends, 0,
            "no span ends should be recorded from within flush"
        );
    }

    /// Events emitted outside any flush span are captured normally.
    #[test]
    fn normal_events_are_captured() {
        let (layer, mut rx) = DbLogLayer::channel();
        let subscriber = tracing_subscriber::registry().with(layer);
        let dispatcher = Dispatch::new(subscriber);

        let _guard = dispatcher.clone().set_default();

        tracing::info!("first");
        tracing::warn!("second");

        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 2);
    }

    /// A span created within another span must record the parent's tracing id.
    #[test]
    fn parent_child_span_relationship() {
        let (layer, mut rx) = DbLogLayer::channel();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = Dispatch::new(subscriber).set_default();

        let parent = tracing::info_span!("parent");
        let _parent_guard = parent.enter();

        let child = tracing::info_span!("child");
        let _child_guard = child.enter();

        drop(_child_guard);
        drop(_parent_guard);

        let mut span_starts = Vec::new();
        while let Ok(record) = rx.try_recv() {
            if let Record::SpanStart(s) = record {
                span_starts.push(s);
            }
        }

        assert_eq!(span_starts.len(), 2);
        // Records arrive in creation order: parent first, child second.
        let parent = &span_starts[0];
        let child = &span_starts[1];
        assert_eq!(parent.name, "parent");
        assert_eq!(child.name, "child");
        assert_eq!(
            child.parent_tracing_id,
            Some(parent.tracing_id),
            "child must reference parent's tracing id"
        );
    }

    /// A root span (no parent entered) must have `parent_tracing_id = None`.
    #[test]
    fn root_span_has_no_parent() {
        let (layer, mut rx) = DbLogLayer::channel();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = Dispatch::new(subscriber).set_default();

        let span = tracing::info_span!("root");
        let _guard = span.enter();
        drop(_guard);

        while let Ok(record) = rx.try_recv() {
            if let Record::SpanStart(s) = record {
                assert_eq!(s.name, "root");
                assert!(s.parent_tracing_id.is_none(), "root span has no parent");
                return;
            }
        }
        panic!("no SpanStart record received");
    }

    /// Fields recorded after span creation via `span.record(...)` must produce
    /// a `SpanFields` record that the writer merges into the span's fields.
    #[test]
    fn late_span_fields_are_captured() {
        use tracing::field;

        let (layer, mut rx) = DbLogLayer::channel();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = Dispatch::new(subscriber).set_default();

        // `late_field` is declared as Empty so it can be recorded later.
        let span = tracing::info_span!("my_span", initial = 42, late_field = field::Empty);
        let _guard = span.enter();
        span.record("late_field", "hello");
        drop(_guard);

        let mut found_start = false;
        let mut found_fields = false;
        while let Ok(record) = rx.try_recv() {
            match record {
                Record::SpanStart(s) => {
                    assert_eq!(s.name, "my_span");
                    let fields = &s.fields;
                    assert_eq!(
                        fields.get("initial"),
                        Some(&serde_json::json!(42)),
                        "initial field should be 42: {fields:?}"
                    );
                    assert_eq!(
                        fields.get("late_field"),
                        Some(&serde_json::Value::Null),
                        "late_field (Empty) should be null in span start: {fields:?}"
                    );
                    found_start = true;
                }
                Record::SpanFields(f) => {
                    assert!(
                        f.fields.contains_key("late_field"),
                        "late_field should be in span fields: {:?}",
                        f.fields
                    );
                    assert_eq!(
                        f.fields.get("late_field"),
                        Some(&serde_json::Value::String("hello".to_owned())),
                        "late_field value should be \"hello\""
                    );
                    found_fields = true;
                }
                _ => {}
            }
        }
        assert!(found_start, "SpanStart record must be present");
        assert!(found_fields, "SpanFields record must be present");
    }

    /// A span declared with only `field::Empty` placeholders must still
    /// produce a non-empty fields map (with null values) so that the writer
    /// creates a `span_fields` entry. Without this, late-recorded values are
    /// silently dropped.
    #[test]
    fn empty_only_span_produces_null_fields() {
        use tracing::field;

        let (layer, mut rx) = DbLogLayer::channel();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = Dispatch::new(subscriber).set_default();

        let span = tracing::info_span!("req", user_id = field::Empty, status = field::Empty);
        let _guard = span.enter();
        span.record("status", "ok");
        drop(_guard);

        let mut found_start = false;
        let mut found_fields = false;
        while let Ok(record) = rx.try_recv() {
            match record {
                Record::SpanStart(s) => {
                    let fields = &s.fields;
                    assert_eq!(
                        fields.get("user_id"),
                        Some(&serde_json::Value::Null),
                        "user_id (Empty) should be null"
                    );
                    assert_eq!(
                        fields.get("status"),
                        Some(&serde_json::Value::Null),
                        "status (Empty) should be null at start"
                    );
                    found_start = true;
                }
                Record::SpanFields(f) => {
                    assert_eq!(
                        f.fields.get("status"),
                        Some(&serde_json::Value::String("ok".to_owned())),
                        "late-recorded status should be \"ok\""
                    );
                    found_fields = true;
                }
                _ => {}
            }
        }
        assert!(found_start, "SpanStart record must be present");
        assert!(
            found_fields,
            "SpanFields record must be present for late-recorded values"
        );
    }
}
