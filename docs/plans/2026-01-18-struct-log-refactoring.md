# struct-log Refactoring Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor struct-log to remove tracing-bunyan-formatter dependency, add proper error handling, optimize performance, and improve API with builder pattern.

**Architecture:** Replace external JsonStorage with custom SpanFieldsStorage. Wrap setup logic in StructLogBuilder with typed errors. Use thread-local buffer pool and static level strings for performance.

**Tech Stack:** Rust, tracing, tracing-subscriber, serde_json

---

### Task 1: Create error.rs with SetupError

**Files:**
- Create: `lib/struct-log/src/error.rs`
- Modify: `lib/struct-log/src/lib.rs`

**Step 1: Create error module**

Create `lib/struct-log/src/error.rs`:

```rust
use std::fmt;

/// Errors that can occur during logger setup
#[derive(Debug)]
pub enum SetupError {
    /// LogTracer already initialized (log -> tracing bridge)
    LogTracerAlreadyInitialized,
    /// Global subscriber already set
    SubscriberAlreadySet,
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LogTracerAlreadyInitialized => {
                write!(f, "log tracer already initialized")
            }
            Self::SubscriberAlreadySet => {
                write!(f, "global tracing subscriber already set")
            }
        }
    }
}

impl std::error::Error for SetupError {}
```

**Step 2: Add module to lib.rs**

Modify `lib/struct-log/src/lib.rs` - add after line 1:

```rust
mod error;

pub use error::SetupError;
```

**Step 3: Verify it compiles**

Run: `cargo check -p struct-log`
Expected: success

**Step 4: Commit**

```bash
git add lib/struct-log/src/error.rs lib/struct-log/src/lib.rs
git commit -m "feat(struct-log): add SetupError type"
```

---

### Task 2: Create storage.rs with SpanFieldsStorage

**Files:**
- Create: `lib/struct-log/src/storage.rs`
- Modify: `lib/struct-log/src/lib.rs`

**Step 1: Create storage module**

Create `lib/struct-log/src/storage.rs`:

```rust
use serde_json::Value;
use std::collections::HashMap;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// Storage for span fields, used to pass context to log events
#[derive(Default)]
pub struct SpanFieldsStorage {
    fields: HashMap<&'static str, Value>,
}

impl SpanFieldsStorage {
    pub fn values(&self) -> &HashMap<&'static str, Value> {
        &self.fields
    }
}

impl Visit for SpanFieldsStorage {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name(), Value::String(format!("{:?}", value)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name(), Value::String(value.to_owned()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name(), Value::Number(value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name(), Value::Number(value.into()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name(), Value::Bool(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if let Some(number) = serde_json::Number::from_f64(value) {
            self.fields.insert(field.name(), Value::Number(number));
        } else {
            self.fields
                .insert(field.name(), Value::String(value.to_string()));
        }
    }
}

/// Layer that stores span fields in extensions for later retrieval
pub struct StorageLayer;

impl<S> Layer<S> for StorageLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &tracing::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut storage = SpanFieldsStorage::default();
            attrs.record(&mut storage);
            span.extensions_mut().insert(storage);
        }
    }

    fn on_record(&self, id: &tracing::Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            if let Some(storage) = span.extensions_mut().get_mut::<SpanFieldsStorage>() {
                values.record(storage);
            }
        }
    }
}
```

**Step 2: Add module to lib.rs**

Modify `lib/struct-log/src/lib.rs` - add after error module:

```rust
mod storage;

pub use storage::{SpanFieldsStorage, StorageLayer};
```

**Step 3: Verify it compiles**

Run: `cargo check -p struct-log`
Expected: success

**Step 4: Commit**

```bash
git add lib/struct-log/src/storage.rs lib/struct-log/src/lib.rs
git commit -m "feat(struct-log): add SpanFieldsStorage and StorageLayer"
```

---

### Task 3: Update JsonLogLayer to use SpanFieldsStorage

**Files:**
- Modify: `lib/struct-log/src/formatting_layer.rs`

**Step 1: Update imports and constants**

Replace lines 1-41 in `lib/struct-log/src/formatting_layer.rs`:

```rust
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
```

**Step 2: Update JsonLogLayer impl**

Replace the `impl<W: for<'a> MakeWriter<'a> + 'static> JsonLogLayer<W>` block (lines 43-81):

```rust
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
```

**Step 3: Update Layer impl to use SpanFieldsStorage**

Replace the `impl<S, W> Layer<S> for JsonLogLayer<W>` block (lines 82-141):

```rust
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
```

**Step 4: Verify it compiles**

Run: `cargo check -p struct-log`
Expected: success

**Step 5: Commit**

```bash
git add lib/struct-log/src/formatting_layer.rs
git commit -m "refactor(struct-log): use SpanFieldsStorage instead of bunyan JsonStorage"
```

---

### Task 4: Update setup.rs with Result and use StorageLayer

**Files:**
- Modify: `lib/struct-log/src/setup.rs`

**Step 1: Update setup.rs**

Replace entire `lib/struct-log/src/setup.rs`:

```rust
use std::env;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::error::SetupError;
use crate::formatting_layer::JsonLogLayer;
use crate::storage::StorageLayer;

pub fn setup_logger(
    application_name: String,
    version: String,
) -> Result<Option<WorkerGuard>, SetupError> {
    if !env::var("JSON_LOG").is_ok_and(|s| s.parse().unwrap_or_default()) {
        tracing_subscriber::fmt::init();
        return Ok(None);
    }

    LogTracer::init().map_err(|_| SetupError::LogTracerAlreadyInitialized)?;

    let (non_blocking_writer, guard) = tracing_appender::non_blocking(std::io::stdout());

    let formatting_layer = JsonLogLayer::new(application_name, version, non_blocking_writer);
    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env())
        .with(StorageLayer)
        .with(formatting_layer);

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| SetupError::SubscriberAlreadySet)?;

    Ok(Some(guard))
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p struct-log`
Expected: success

**Step 3: Commit**

```bash
git add lib/struct-log/src/setup.rs
git commit -m "refactor(struct-log): setup_logger returns Result, uses StorageLayer"
```

---

### Task 5: Create builder.rs

**Files:**
- Create: `lib/struct-log/src/builder.rs`
- Modify: `lib/struct-log/src/lib.rs`

**Step 1: Create builder module**

Create `lib/struct-log/src/builder.rs`:

```rust
use std::env;
use std::io;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::error::SetupError;
use crate::formatting_layer::JsonLogLayer;
use crate::storage::StorageLayer;

/// Builder for configuring structured JSON logging
pub struct StructLogBuilder {
    application: String,
    version: String,
    hostname: Option<String>,
    json_enabled: bool,
}

impl StructLogBuilder {
    /// Create a new builder with required application name and version
    pub fn new(application: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            application: application.into(),
            version: version.into(),
            hostname: None,
            json_enabled: true,
        }
    }

    /// Set a custom hostname (defaults to system hostname)
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Enable or disable JSON output (defaults to true)
    pub fn json_enabled(mut self, enabled: bool) -> Self {
        self.json_enabled = enabled;
        self
    }

    /// Read JSON_LOG env var to determine if JSON should be enabled
    pub fn json_from_env(mut self) -> Self {
        self.json_enabled = env::var("JSON_LOG").is_ok_and(|s| s.parse().unwrap_or_default());
        self
    }

    /// Initialize the logger with the configured settings
    pub fn init(self) -> Result<Option<WorkerGuard>, SetupError> {
        if !self.json_enabled {
            tracing_subscriber::fmt::init();
            return Ok(None);
        }

        LogTracer::init().map_err(|_| SetupError::LogTracerAlreadyInitialized)?;

        let (non_blocking, guard) = tracing_appender::non_blocking(io::stdout());

        let layer = match self.hostname {
            Some(hostname) => {
                JsonLogLayer::with_hostname(self.application, self.version, hostname, non_blocking)
            }
            None => JsonLogLayer::new(self.application, self.version, non_blocking),
        };

        let subscriber = Registry::default()
            .with(EnvFilter::from_default_env())
            .with(StorageLayer)
            .with(layer);

        tracing::subscriber::set_global_default(subscriber)
            .map_err(|_| SetupError::SubscriberAlreadySet)?;

        Ok(Some(guard))
    }
}
```

**Step 2: Add module and export to lib.rs**

Update `lib/struct-log/src/lib.rs` to final form:

```rust
mod builder;
mod error;
mod formatting_layer;
mod setup;
mod storage;

pub use builder::StructLogBuilder;
pub use error::SetupError;
pub use formatting_layer::JsonLogLayer;
pub use setup::setup_logger;
pub use storage::{SpanFieldsStorage, StorageLayer};
```

**Step 3: Verify it compiles**

Run: `cargo check -p struct-log`
Expected: success

**Step 4: Commit**

```bash
git add lib/struct-log/src/builder.rs lib/struct-log/src/lib.rs
git commit -m "feat(struct-log): add StructLogBuilder for flexible configuration"
```

---

### Task 6: Update tests - remove lazy_static

**Files:**
- Modify: `lib/struct-log/tests/e2e.rs`
- Modify: `lib/struct-log/Cargo.toml`

**Step 1: Update e2e.rs**

Replace entire `lib/struct-log/tests/e2e.rs`:

```rust
use std::sync::Mutex;

use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use tracing::{error, info, span, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

use struct_log::{JsonLogLayer, StorageLayer};

use crate::mock_writer::MockWriter;

mod mock_writer;

/// Tests have to be run on a single thread because we are re-using the same buffer for
/// all of them.
static BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

// Run a closure and collect the output emitted by the tracing instrumentation using an in-memory buffer.
fn run_and_get_raw_output<F: Fn()>(action: F) -> String {
    let formatting_layer = JsonLogLayer::new("test-app".into(), "e2e".to_string(), || {
        MockWriter::new(&BUFFER)
    });
    let subscriber = Registry::default()
        .with(StorageLayer)
        .with(formatting_layer);
    tracing::subscriber::with_default(subscriber, action);

    // Return the formatted output as a string to make assertions against
    let mut buffer = BUFFER.lock().unwrap();
    let output = buffer.to_vec();
    // Clean the buffer to avoid cross-tests interactions
    buffer.clear();
    String::from_utf8(output).unwrap()
}

// Run a closure and collect the output emitted by the tracing instrumentation using
// an in-memory buffer as structured new-line-delimited JSON.
fn run_and_get_output<F: Fn()>(action: F) -> Vec<Value> {
    run_and_get_raw_output(action)
        .lines()
        .filter(|&l| !l.is_empty())
        .inspect(|l| println!("{}", l))
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

// Instrumented code to be run to test the behaviour of the tracing instrumentation.
fn test_action() {
    let span = span!(Level::TRACE, "my span", span_field = "quux");
    let _enter = span.enter();

    let inner_span = span!(Level::TRACE, "my inner span", inner_span_field = "quuux");
    inner_span.follows_from(&span);
    let _inner_span_enter = inner_span.enter();

    info!("foo");
    error!(foo = "baz", "bar");
}

#[test]
fn each_line_is_valid_json() {
    let tracing_output = run_and_get_raw_output(test_action);

    // Each line is valid JSON
    for line in tracing_output.lines().filter(|&l| !l.is_empty()) {
        assert!(serde_json::from_str::<Value>(line).is_ok());
    }
}

#[test]
fn each_line_has_the_base_fields() {
    let tracing_output = run_and_get_output(test_action);

    for record in tracing_output {
        println!("{}", record);
        assert!(record.get("runtime").is_some());
        assert!(record.get("level").is_some());
        assert!(record.get("date").is_some());
        assert!(record.get("message").is_some());
        assert!(record.get("file").is_some());
        assert!(record.get("lineno").is_some());
        assert!(record.get("logger").is_some());
        assert!(record.get("hostname").is_some());
        assert!(record.get("version").is_some());
        assert!(record.get("application").is_some());
        assert_eq!(
            record.get("span_field"),
            Some(&Value::String("quux".to_owned()))
        );
        assert_eq!(
            record.get("message_type"),
            Some(&Value::String("app".to_owned()))
        );
        assert_eq!(
            record.get("inner_span_field"),
            Some(&Value::String("quuux".to_owned()))
        );
    }
}

#[test]
fn time_is_formatted_according_to_rfc_3339() {
    let tracing_output = run_and_get_output(test_action);

    for record in tracing_output {
        let time = record.get("date").unwrap().as_str().unwrap();
        let parsed = time::OffsetDateTime::parse(time, &Rfc3339);
        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();
        assert!(parsed.offset().is_utc());
    }
}
```

**Step 2: Remove lazy_static from Cargo.toml**

Remove line `lazy_static = "1.4"` from `[dev-dependencies]` in `lib/struct-log/Cargo.toml`.

**Step 3: Run tests**

Run: `cargo test -p struct-log`
Expected: 3 tests pass

**Step 4: Commit**

```bash
git add lib/struct-log/tests/e2e.rs lib/struct-log/Cargo.toml
git commit -m "refactor(struct-log): remove lazy_static, use static Mutex"
```

---

### Task 7: Remove tracing-bunyan-formatter dependency

**Files:**
- Modify: `lib/struct-log/Cargo.toml`

**Step 1: Remove dependency**

Remove line `tracing-bunyan-formatter = "0.3"` from `[dependencies]` in `lib/struct-log/Cargo.toml`.

**Step 2: Run tests**

Run: `cargo test -p struct-log`
Expected: 3 tests pass

**Step 3: Commit**

```bash
git add lib/struct-log/Cargo.toml
git commit -m "chore(struct-log): remove tracing-bunyan-formatter dependency"
```

---

### Task 8: Bump version and update example

**Files:**
- Modify: `lib/struct-log/Cargo.toml`
- Modify: `example/src/main.rs` (if it uses setup_logger)

**Step 1: Bump version in Cargo.toml**

Change `version = "0.5.0"` to `version = "0.6.0"` in `lib/struct-log/Cargo.toml`.

**Step 2: Check if example needs update**

Run: `grep -n "setup_logger" example/src/main.rs`
If found, update to handle Result (add `?` or `.unwrap()`).

**Step 3: Run full workspace tests**

Run: `cargo test`
Expected: all tests pass

**Step 4: Run clippy**

Run: `cargo clippy -p struct-log`
Expected: no warnings

**Step 5: Commit**

```bash
git add lib/struct-log/Cargo.toml example/src/main.rs
git commit -m "chore(struct-log): bump version to 0.6.0"
```

---

### Task 9: Final verification

**Step 1: Run all tests**

Run: `cargo test`
Expected: all pass

**Step 2: Run clippy on workspace**

Run: `cargo clippy`
Expected: no errors

**Step 3: Build release**

Run: `cargo build --release`
Expected: success

**Step 4: Verify dependencies removed**

Run: `cargo tree -p struct-log | grep -E "bunyan|lazy_static"`
Expected: no output (dependencies removed)
