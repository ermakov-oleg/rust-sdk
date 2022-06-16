use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;
use std::io::Write;
use time::format_description::well_known::Rfc3339;
use tracing::{Event, Subscriber};
use tracing_bunyan_formatter::JsonStorage;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

pub struct JsonLogLayer<W: for<'a> MakeWriter<'a> + 'static> {
    make_writer: W,
    hostname: String,
    version: String,
    application: String,
}

const DATE: &str = "date";
const MESSAGE_TYPE: &str = "message_type"; // todo: set type!
const RUNTIME: &str = "runtime";
const APPLICATION: &str = "application";
const LEVEL: &str = "level";
const HOSTNAME: &str = "container_id";
const MESSAGE: &str = "message";
const LOGGER: &str = "logger";
const LINENO: &str = "lineno";
const FILE: &str = "file";
const VERSION: &str = "version";

const RESERVED_FIELDS: [&str; 11] = [
    DATE,
    MESSAGE_TYPE,
    RUNTIME,
    APPLICATION,
    LEVEL,
    HOSTNAME,
    MESSAGE,
    LOGGER,
    LINENO,
    FILE,
    VERSION,
];

impl<W: for<'a> MakeWriter<'a> + 'static> JsonLogLayer<W> {
    pub fn new(application: String, version: String, make_writer: W) -> Self {
        Self {
            make_writer,
            application,
            version,
            hostname: gethostname::gethostname().to_string_lossy().into_owned(),
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
        map_serializer.serialize_entry(
            LEVEL,
            &format!("{}", event.metadata().level()).to_lowercase(),
        )?;
        map_serializer.serialize_entry(LOGGER, event.metadata().target())?;
        map_serializer.serialize_entry(LINENO, &event.metadata().line())?;
        map_serializer.serialize_entry(FILE, &event.metadata().file())?;
        map_serializer.serialize_entry(MESSAGE, &message)?;
        Ok(())
    }

    fn emit(&self, mut buffer: Vec<u8>) -> Result<(), std::io::Error> {
        buffer.write_all(b"\n")?;
        self.make_writer.make_writer().write_all(&buffer)
    }
}
impl<S, W> Layer<S> for JsonLogLayer<W>
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    W: for<'a> MakeWriter<'a> + 'static,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut event_visitor = JsonStorage::default();
        let current_span = ctx.lookup_current();
        event.record(&mut event_visitor);

        let format = || {
            let mut buffer = Vec::new();

            let mut serializer = serde_json::Serializer::new(&mut buffer);
            let mut map_serializer = serializer.serialize_map(None)?;

            let message = format_event_message(event, &event_visitor);
            self.serialize_core_fields(&mut map_serializer, &message, event)?;

            // Add all the other fields associated with the event, expect the message we already used.
            for (key, value) in event_visitor
                .values()
                .iter()
                .filter(|(&key, _)| !RESERVED_FIELDS.contains(&key))
            {
                map_serializer.serialize_entry(key, value)?;
            }

            // Add all the fields from the current span, if we have one.
            if let Some(span) = &current_span {
                let extensions = span.extensions();
                if let Some(visitor) = extensions.get::<JsonStorage>() {
                    for (key, value) in visitor.values() {
                        if !RESERVED_FIELDS.contains(key) {
                            map_serializer.serialize_entry(key, value)?;
                        }
                    }
                }
            }

            map_serializer.end()?;
            Ok(buffer)
        };

        let result: std::io::Result<Vec<u8>> = format();
        if let Ok(formatted) = result {
            let _ = self.emit(formatted);
        }
    }
}

fn format_event_message(event: &Event, event_visitor: &JsonStorage<'_>) -> String {
    // Extract the "message" field, if provided. Fallback to the target, if missing.
    let message = event_visitor
        .values()
        .get("message")
        .map(|v| match v {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })
        .flatten()
        .unwrap_or_else(|| event.metadata().target())
        .to_owned();

    message
}
