//! A `tracing_subscriber` layer that persists spans and events to the database.
//!
//! The layer captures tracing records and sends them through a bounded channel
//! to a background task that batch-inserts them via [`LogWriter`]. If the
//! channel is full, records are dropped and a synthetic warning event is
//! inserted to record how many were lost.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aperture_storage::{EventRecord, Level, LogWriter, SpanRecord};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes as SpanAttributes, Id as SpanId};
use tracing::{Event, Level as TracingLevel, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

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
    line: Option<i64>,
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
    line: Option<i64>,
    fields: Option<String>,
}

/// Channel capacity. When full, records are dropped.
const CHANNEL_CAPACITY: usize = 4096;

/// How long the background writer waits for more records before flushing.
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Maximum records to batch before flushing.
const FLUSH_BATCH: usize = 128;

/// Target prefixes whose log-bridged events below WARN are dropped before
/// queueing. These crates produce trace-level diagnostics as a side effect
/// of database writes, which would cause a feedback loop if persisted.
///
/// Native tracing events from these targets are already excluded by the
/// `Targets` filter on the DB layer. This list catches the same crates when
/// they emit through the `log` crate, whose bridged events carry the static
/// target `"log"` instead of the real crate target.
const NOISY_LOG_PREFIXES: &[&str] = &["turso", "tantivy", "backhand"];

/// Returns `true` if a log-bridged event from `target` at `level` should be
/// dropped before queueing. Events at WARN or above are always kept.
fn is_noisy_log_event(target: &str, level: &TracingLevel) -> bool {
    if *level >= TracingLevel::WARN {
        return false;
    }
    NOISY_LOG_PREFIXES.iter().any(|p| target.starts_with(p))
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
            line: meta.line().map(|l| l as i64),
            started_at: now(),
            fields: visitor.into_json(),
        }));
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = FieldCollector::new(&self.boot_id);
        event.record(&mut visitor);

        // For log-bridged events the metadata target is "log". The real
        // target lives in the `log.target` field. Use whichever is
        // available.
        let target = visitor
            .log_target()
            .map(str::to_owned)
            .unwrap_or_else(|| meta.target().to_owned());

        // Drop noisy log-bridged events from database-engine crates before
        // they enter the channel to prevent feedback loops.
        if is_noisy_log_event(&target, meta.level()) {
            return;
        }

        let span_tracing_id = ctx.event_span(event).map(|s| s.id().into_u64());

        let file = visitor
            .take_log_file()
            .or_else(|| meta.file().map(str::to_owned));
        let line = visitor
            .take_log_line()
            .or_else(|| meta.line().map(|l| l as i64));

        self.try_send(Record::Event(EventMsg {
            span_tracing_id,
            level: tracing_to_db_level(meta.level()),
            target,
            message: visitor.take_message(),
            timestamp: now(),
            file,
            line,
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

/// Flushes a batch of records to the database using prepared statements.
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
async fn flush_dropped(writer: &mut LogWriter, dropped: &AtomicU64) {
    let count = dropped.swap(0, Ordering::Relaxed);
    if count > 0 {
        let _ = writer.record_dropped(count, now()).await;
    }
}

/// Field visitor that collects all fields into a JSON string and extracts the
/// "message" field specially.
///
/// Writes JSON directly to a `String` for efficiency, avoiding an
/// intermediate `Map<String, Value>`.
struct FieldCollector {
    json: String,
    first: bool,
    message: Option<String>,
    /// Real target from a `log`-bridged event (`log.target` field).
    log_target: Option<String>,
    /// Source file from a `log`-bridged event (`log.file` field).
    log_file: Option<String>,
    /// Source line from a `log`-bridged event (`log.line` field).
    log_line: Option<i64>,
}

impl FieldCollector {
    fn new(boot_id: &Uuid) -> Self {
        let mut json = String::new();
        let _ = write!(json, "{{\"boot_id\":");
        write_json_string(&mut json, &boot_id.to_string());
        Self {
            json,
            first: false,
            message: None,
            log_target: None,
            log_file: None,
            log_line: None,
        }
    }

    fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// Returns the real target from a `log`-bridged event, if present.
    fn log_target(&self) -> Option<&str> {
        self.log_target.as_deref()
    }

    fn take_log_file(&mut self) -> Option<String> {
        self.log_file.take()
    }

    fn take_log_line(&mut self) -> Option<i64> {
        self.log_line.take()
    }

    fn into_json(mut self) -> Option<String> {
        if self.first {
            None
        } else {
            self.json.push('}');
            Some(self.json)
        }
    }

    fn write_key(&mut self, name: &str) {
        if self.first {
            self.first = false;
            self.json.push('{');
        } else {
            self.json.push(',');
        }
        write_json_string(&mut self.json, name);
        self.json.push(':');
    }

    fn store_str(&mut self, name: &str, value: &str) {
        match name {
            // `message` is a dedicated column on the events table, so it is
            // not also recorded as a structured field.
            "message" => self.message = Some(value.to_owned()),
            "log.target" => self.log_target = Some(value.to_owned()),
            "log.file" => self.log_file = Some(value.to_owned()),
            "log.module_path" => {}
            _ => {
                self.write_key(name);
                write_json_string(&mut self.json, value);
            }
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name().starts_with("log.") {
            return;
        }
        let s = format!("{value:?}");
        self.store_str(field.name(), &s);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.store_str(field.name(), value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.write_key(field.name());
        self.json.push_str(if value { "true" } else { "false" });
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.write_key(field.name());
        write!(self.json, "{value}").unwrap();
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "log.line" {
            self.log_line = Some(value as i64);
        } else {
            self.write_key(field.name());
            write!(self.json, "{value}").unwrap();
        }
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.write_key(field.name());
        write!(self.json, "{value}").unwrap();
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.write_key(field.name());
        write!(self.json, "{value}").unwrap();
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.write_key(field.name());
        if let Some(n) = serde_json::Number::from_f64(value) {
            write!(self.json, "{n}").unwrap();
        } else {
            self.json.push_str("null");
        }
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        let hex: String = value.iter().map(|b| format!("{b:02x}")).collect();
        self.store_str(field.name(), &hex);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        // Record the entire source chain as a JSON array under the field name.
        // The head entry is always the Display of `value`; each subsequent
        // entry is the Display of `Error::source` walking down the chain.
        // The same field name is reused so callers using `error = &err` still
        // find the value under `error`, just shaped as an array.
        self.write_key(field.name());
        self.json.push('[');
        let mut current: Option<&(dyn Error + 'static)> = Some(value);
        let mut first = true;
        while let Some(error) = current {
            if !first {
                self.json.push(',');
            }
            first = false;
            write_json_string(&mut self.json, &format!("{error}"));
            current = error.source();
        }
        self.json.push(']');
    }
}

/// Writes a JSON-escaped string into `out`.
fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
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
