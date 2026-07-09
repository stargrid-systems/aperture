use std::error::Error;
use std::fmt::{self, Debug};
use std::io;

use serde::ser::{SerializeMap as _, SerializeSeq as _};
use serde_json::{Map, Value};
use tracing::field::{Field, FieldSet, Visit};

/// Field visitor that collects tracing fields into a [`serde_json::Map`] and
/// extracts special fields like "message" and log-bridged metadata.
pub struct FieldCollector {
    fields: Map<String, Value>,
    message: Option<String>,
    /// Real target from a `log`-bridged event (`log.target` field).
    log_target: Option<String>,
    /// Source file from a `log`-bridged event (`log.file` field).
    log_file: Option<String>,
    /// Source line from a `log`-bridged event (`log.line` field).
    log_line: Option<u32>,
}

impl FieldCollector {
    /// Creates a collector pre-populated with every declared field set to
    /// `null`. The visitor overwrites entries as real values arrive. Fields
    /// declared as `field::Empty` stay `null`. Fields whose values are
    /// consumed into dedicated fields (message, log.*) are excluded so they
    /// never appear in the output map.
    pub fn new(fields: &FieldSet) -> Self {
        let mut map = Map::with_capacity(fields.len());
        for field in fields {
            let name = field.name();
            if !is_consumed(name) {
                map.insert(name.to_owned(), Value::Null);
            }
        }
        Self {
            fields: map,
            message: None,
            log_target: None,
            log_file: None,
            log_line: None,
        }
    }

    /// Creates a collector for additional fields recorded after span creation
    /// (via `Span::record`).
    pub fn additional() -> Self {
        Self {
            fields: Map::new(),
            message: None,
            log_target: None,
            log_file: None,
            log_line: None,
        }
    }

    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    pub fn take_log_target(&mut self) -> Option<String> {
        self.log_target.take()
    }

    pub fn take_log_file(&mut self) -> Option<String> {
        self.log_file.take()
    }

    pub fn take_log_line(&mut self) -> Option<u32> {
        self.log_line.take()
    }

    pub fn into_fields(self) -> Option<Map<String, Value>> {
        if self.fields.is_empty() {
            None
        } else {
            Some(self.fields)
        }
    }

    fn collect_str<S: fmt::Display>(&mut self, name: &str, value: S) {
        match name {
            "message" => self.message = Some(value.to_string()),
            "log.target" => self.log_target = Some(value.to_string()),
            "log.file" => self.log_file = Some(value.to_string()),
            "log.module_path" => {}
            _ => {
                self.fields
                    .insert(name.to_owned(), Value::String(value.to_string()));
            }
        }
    }

    fn collect_integer<N: Copy + serde::Serialize + TryInto<u32>>(&mut self, name: &str, value: N) {
        if name == "log.line"
            && let Ok(line) = value.try_into()
        {
            self.log_line = Some(line);
        } else {
            self.fields
                .insert(name.to_owned(), serde_json::to_value(value).unwrap());
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
        self.fields
            .insert(field.name().to_owned(), Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.collect_integer(field.name(), value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.collect_integer(field.name(), value);
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        // String: serde_json numbers cannot hold the full i128 range.
        self.fields
            .insert(field.name().to_owned(), Value::String(value.to_string()));
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        // String: serde_json numbers cannot hold the full u128 range.
        self.fields
            .insert(field.name().to_owned(), Value::String(value.to_string()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let v = serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string()));
        self.fields.insert(field.name().to_owned(), v);
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        self.collect_str(field.name(), DisplayAsHex(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        self.fields.insert(
            field.name().to_owned(),
            serde_json::to_value(ErrorChainSerializer(value)).unwrap(),
        );
    }
}

/// Whether `name` is a field whose value is extracted into a dedicated field
/// on [`FieldCollector`] rather than stored in the output map.
fn is_consumed(name: &str) -> bool {
    matches!(
        name,
        "message" | "log.target" | "log.file" | "log.module_path" | "log.line"
    )
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
