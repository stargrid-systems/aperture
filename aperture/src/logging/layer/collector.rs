//! A `tracing_subscriber` layer that persists spans and events to the database.
//!
//! The layer captures tracing records and sends them through a bounded channel
//! to a background task that batch-inserts them via [`LogWriter`]. If the
//! channel is full, records are dropped and a synthetic warning event is
//! inserted to record how many were lost.

use std::error::Error;
use std::fmt::Debug;
use std::{fmt, io};

use serde::ser::{SerializeMap as _, SerializeSeq as _};
use tracing::field::{Field, Visit};
use uuid::Uuid;

/// Field visitor that collects all fields into a JSON byte buffer and
/// extracts the "message" field specially.
///
/// Writes JSON directly via `serde_json::to_writer` into a `Vec<u8>`, avoiding
/// intermediate allocations.
pub struct FieldCollector {
    json: Vec<u8>,
    first: bool,
    message: Option<String>,
    /// Real target from a `log`-bridged event (`log.target` field).
    log_target: Option<String>,
    /// Source file from a `log`-bridged event (`log.file` field).
    log_file: Option<String>,
    /// Source line from a `log`-bridged event (`log.line` field).
    log_line: Option<u32>,
}

impl FieldCollector {
    pub fn new(boot_id: &Uuid) -> Self {
        let mut json = Vec::new();
        json.extend_from_slice(b"{\"boot_id\":");
        serde_json::to_writer(&mut json, boot_id).unwrap();
        Self {
            json,
            first: false,
            message: None,
            log_target: None,
            log_file: None,
            log_line: None,
        }
    }

    /// Creates a collector for additional fields recorded after span creation
    /// (via `Span::record`). Does not include `boot_id`.
    pub fn additional() -> Self {
        Self {
            json: Vec::new(),
            first: true,
            message: None,
            log_target: None,
            log_file: None,
            log_line: None,
        }
    }

    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// Returns the real target from a `log`-bridged event, if present.
    pub fn take_log_target(&mut self) -> Option<String> {
        self.log_target.take()
    }

    pub fn take_log_file(&mut self) -> Option<String> {
        self.log_file.take()
    }

    pub fn take_log_line(&mut self) -> Option<u32> {
        self.log_line.take()
    }

    pub fn into_json(mut self) -> Option<String> {
        if self.first {
            None
        } else {
            self.json.push(b'}');
            Some(String::from_utf8(self.json).expect("buffer is valid UTF-8"))
        }
    }

    fn write_key(&mut self, name: &str) {
        if self.first {
            self.first = false;
            self.json.push(b'{');
        } else {
            self.json.push(b',');
        }
        serde_json::to_writer(&mut self.json, name).unwrap();
        self.json.push(b':');
    }

    fn collect_str<S: fmt::Display>(&mut self, name: &str, value: S) {
        match name {
            "message" => self.message = Some(value.to_string()),
            "log.target" => self.log_target = Some(value.to_string()),
            "log.file" => self.log_file = Some(value.to_string()),
            "log.module_path" => {}
            _ => {
                self.write_key(name);
                serde_json::to_writer(&mut self.json, &SerializeAsDisplay(value)).unwrap();
            }
        }
    }

    fn collect_integer<N: Copy + serde::Serialize + TryInto<u32>>(&mut self, name: &str, value: N) {
        if name == "log.line"
            && let Ok(line) = value.try_into()
        {
            self.log_line = Some(line);
        } else {
            self.write_key(name);
            serde_json::to_writer(&mut self.json, &value).unwrap();
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.collect_str(field.name(), DisplayAsDebug(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.collect_str(field.name(), value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.write_key(field.name());
        serde_json::to_writer(&mut self.json, &value).unwrap();
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.collect_integer(field.name(), value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.collect_integer(field.name(), value);
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.collect_integer(field.name(), value);
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.collect_integer(field.name(), value);
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.write_key(field.name());
        if let Some(n) = serde_json::Number::from_f64(value) {
            serde_json::to_writer(&mut self.json, &n).unwrap();
        } else {
            serde_json::to_writer(&mut self.json, &value.to_string()).unwrap();
        }
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        self.collect_str(field.name(), DisplayAsHex(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        self.write_key(field.name());
        serde_json::to_writer(&mut self.json, &ErrorChainSerializer(value)).unwrap();
    }
}

struct ErrorChainSerializer<'a>(&'a (dyn Error + 'static));

impl serde::Serialize for ErrorChainSerializer<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_seq(None)?;
        let mut current: Option<&(dyn Error + 'static)> = Some(self.0);
        while let Some(error) = current {
            seq.serialize_element(&ErrorSerializer(error))?;
            current = error.source();
        }
        seq.end()
    }
}

struct ErrorSerializer<'a>(&'a (dyn Error + 'static));

impl serde::Serialize for ErrorSerializer<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        if let Some(io_err) = self.0.downcast_ref::<io::Error>() {
            map.serialize_entry("type", "io")?;
            map.serialize_entry("kind", &SerializeAsDisplay(DisplayAsDebug(io_err.kind())))?;
        } else {
            map.serialize_entry("type", "generic")?;
        }
        map.serialize_entry("message", &SerializeAsDisplay(self.0))?;
        map.end()
    }
}

struct SerializeAsDisplay<T>(T);

impl<T: fmt::Display> serde::Serialize for SerializeAsDisplay<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

struct DisplayAsDebug<T>(T);

impl<T: fmt::Debug> fmt::Display for DisplayAsDebug<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

struct DisplayAsHex<T>(T);

impl<T: AsRef<[u8]>> fmt::Display for DisplayAsHex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0
            .as_ref()
            .iter()
            .try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}
