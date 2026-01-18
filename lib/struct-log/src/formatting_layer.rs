use crate::storage::SpanFieldsStorage;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;
use std::cell::RefCell;
use std::io::Write;
use time::format_description::well_known::Rfc3339;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

pub struct JsonLogLayer<W: for<'a> MakeWriter<'a> + 'static> {
    make_writer: W,
    hostname: String,
    version: String,
    application: String,
}

const DATE: &str = "date";
const RUNTIME: &str = "runtime";
const APPLICATION: &str = "application";
const LEVEL: &str = "level";
const HOSTNAME: &str = "hostname";
const MESSAGE: &str = "message";
const LOGGER: &str = "logger";
const LINENO: &str = "lineno";
const FILE: &str = "file";
const VERSION: &str = "version";
const MESSAGE_TYPE: &str = "message_type";

const RESERVED_FIELDS: [&str; 11] = [
    DATE,
    RUNTIME,
    APPLICATION,
    LEVEL,
    HOSTNAME,
    MESSAGE,
    LOGGER,
    LINENO,
    FILE,
    VERSION,
    MESSAGE_TYPE,
];

thread_local! {
    static BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn level_to_str(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "trace",
    }
}

impl<W: for<'a> MakeWriter<'a> + 'static> JsonLogLayer<W> {
    pub fn new(application: String, version: String, make_writer: W) -> Self {
        Self {
            make_writer,
            application,
            version,
            hostname: gethostname::gethostname().to_string_lossy().into_owned(),
        }
    }

    pub fn with_hostname(
        application: String,
        version: String,
        hostname: String,
        make_writer: W,
    ) -> Self {
        Self {
            make_writer,
            application,
            version,
            hostname,
        }
    }

    fn serialize_core_fields(
        &self,
        map_serializer: &mut impl SerializeMap<Error = serde_json::Error>,
        message: &str,
        event: &Event,
    ) -> Result<(), std::io::Error> {
        map_serializer.serialize_entry(RUNTIME, "rust")?;
        map_serializer.serialize_entry(APPLICATION, &self.application)?;
        map_serializer.serialize_entry(VERSION, &self.version)?;
        map_serializer.serialize_entry(HOSTNAME, &self.hostname)?;
        if let Ok(date) = &time::OffsetDateTime::now_utc().format(&Rfc3339) {
            map_serializer.serialize_entry(DATE, date)?;
        }
        map_serializer.serialize_entry(LEVEL, level_to_str(event.metadata().level()))?;
        map_serializer.serialize_entry(LOGGER, event.metadata().target())?;
        map_serializer.serialize_entry(LINENO, &event.metadata().line())?;
        map_serializer.serialize_entry(FILE, &event.metadata().file())?;
        map_serializer.serialize_entry(MESSAGE, message)?;
        Ok(())
    }

    fn emit(&self, buffer: &[u8]) -> Result<(), std::io::Error> {
        let mut writer = self.make_writer.make_writer();
        writer.write_all(buffer)?;
        writer.write_all(b"\n")
    }
}

impl<S, W> Layer<S> for JsonLogLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'a> MakeWriter<'a> + 'static,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let current_span = ctx.lookup_current();

        // Collect event fields
        let mut event_storage = SpanFieldsStorage::default();
        event.record(&mut event_storage);

        let format_result: std::io::Result<()> = BUFFER.with(|buf| {
            let mut buf = buf.borrow_mut();
            buf.clear();

            let mut serializer = serde_json::Serializer::new(&mut *buf);
            let mut map_serializer = serializer.serialize_map(None)?;

            let message = format_event_message(event, &event_storage);
            self.serialize_core_fields(&mut map_serializer, &message, event)?;

            let mut message_type_found = false;

            // Add event fields (except reserved)
            for (key, value) in event_storage.values().iter() {
                if *key == MESSAGE_TYPE {
                    message_type_found = true;
                }
                if !RESERVED_FIELDS.contains(key) {
                    map_serializer.serialize_entry(key, value)?;
                }
            }

            // Add span fields
            if let Some(span) = &current_span {
                let extensions = span.extensions();
                if let Some(storage) = extensions.get::<SpanFieldsStorage>() {
                    for (key, value) in storage.values() {
                        if *key == MESSAGE_TYPE {
                            message_type_found = true;
                        }
                        if !RESERVED_FIELDS.contains(key) {
                            map_serializer.serialize_entry(key, value)?;
                        }
                    }
                }
            }

            // Add default message_type if not provided
            if !message_type_found {
                map_serializer.serialize_entry(MESSAGE_TYPE, "app")?;
            }

            map_serializer.end()?;

            self.emit(&buf)
        });

        // Silently ignore errors - logging should not break the application
        let _ = format_result;
    }
}

fn format_event_message(event: &Event, storage: &SpanFieldsStorage) -> String {
    storage
        .values()
        .get("message")
        .and_then(|v| match v {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| event.metadata().target())
        .to_owned()
}
