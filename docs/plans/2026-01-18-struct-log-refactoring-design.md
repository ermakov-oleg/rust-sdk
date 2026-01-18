# struct-log Refactoring Design

## Overview

Рефакторинг библиотеки `struct-log` для улучшения rust-way подхода, удаления лишних зависимостей и оптимизации производительности.

## Changes Summary

1. Убрать `tracing-bunyan-formatter` — написать свой `SpanFieldsStorage` и `StorageLayer`
2. Типизированные ошибки — `setup_logger` возвращает `Result<Option<WorkerGuard>, SetupError>`
3. Оптимизация уровней — статические строки вместо `format!` + `to_lowercase()`
4. Пул буферов — `thread_local!` для переиспользования `Vec<u8>`
5. Builder pattern — конфигурируемый API для `JsonLogLayer`
6. Переименовать hostname — `container_id` → `hostname`
7. Убрать `lazy_static` — использовать `static Mutex` из std
8. Исправить `MESSAGE_TYPE` — добавить в `RESERVED_FIELDS`

## Module Structure

```
src/
  lib.rs              # pub exports
  layer.rs            # JsonLogLayer (переименован из formatting_layer)
  storage.rs          # SpanFieldsStorage + StorageLayer (новый)
  builder.rs          # StructLogBuilder (новый)
  error.rs            # SetupError enum (новый)
  setup.rs            # setup_logger (использует builder внутри)
```

**Public API:**
```rust
// Simple variant (backward compatibility)
pub use setup::setup_logger;

// Advanced variant
pub use builder::StructLogBuilder;
pub use layer::JsonLogLayer;
pub use error::SetupError;
```

## Implementation Details

### storage.rs — SpanFieldsStorage

```rust
use serde_json::Value;
use std::collections::HashMap;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// Span fields storage
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
        self.fields.insert(field.name(), Value::String(format!("{:?}", value)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.insert(field.name(), Value::String(value.to_owned()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(field.name(), Value::Number(value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(field.name(), Value::Number(value.into()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name(), Value::Bool(value));
    }
}

/// Layer for storing fields in span extensions
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

### error.rs — SetupError

```rust
use std::fmt;

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

### builder.rs — StructLogBuilder

```rust
use crate::error::SetupError;
use crate::layer::JsonLogLayer;
use crate::storage::StorageLayer;
use std::io::{self, Write};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub struct StructLogBuilder<W = fn() -> io::Stdout> {
    application: String,
    version: String,
    hostname: Option<String>,
    make_writer: W,
    json_enabled: bool,
}

impl StructLogBuilder<fn() -> io::Stdout> {
    pub fn new(application: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            application: application.into(),
            version: version.into(),
            hostname: None,
            make_writer: io::stdout,
            json_enabled: true,
        }
    }
}

impl<W> StructLogBuilder<W>
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    pub fn writer<W2>(self, make_writer: W2) -> StructLogBuilder<W2> {
        StructLogBuilder {
            application: self.application,
            version: self.version,
            hostname: self.hostname,
            make_writer,
            json_enabled: self.json_enabled,
        }
    }

    pub fn json_enabled(mut self, enabled: bool) -> Self {
        self.json_enabled = enabled;
        self
    }

    pub fn init(self) -> Result<Option<WorkerGuard>, SetupError> {
        if !self.json_enabled {
            tracing_subscriber::fmt::init();
            return Ok(None);
        }

        LogTracer::init().map_err(|_| SetupError::LogTracerAlreadyInitialized)?;

        let (non_blocking, guard) = tracing_appender::non_blocking(io::stdout());

        let layer = JsonLogLayer::new(
            self.application,
            self.version,
            self.hostname,
            non_blocking,
        );

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

### setup.rs — Backward Compatibility

```rust
use crate::builder::StructLogBuilder;
use crate::error::SetupError;
use std::env;
use tracing_appender::non_blocking::WorkerGuard;

pub fn setup_logger(
    application_name: String,
    version: String,
) -> Result<Option<WorkerGuard>, SetupError> {
    let json_enabled = env::var("JSON_LOG")
        .map(|s| s.parse().unwrap_or(false))
        .unwrap_or(false);

    StructLogBuilder::new(application_name, version)
        .json_enabled(json_enabled)
        .init()
}
```

### layer.rs — Optimizations

**Static strings for levels:**

```rust
use tracing::Level;

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

**Thread-local buffer pool:**

```rust
use std::cell::RefCell;

thread_local! {
    static BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(1024));
}

impl<W: for<'a> MakeWriter<'a> + 'static> JsonLogLayer<W> {
    fn format_event(&self, event: &Event, ctx: &Context<'_, impl Subscriber>) -> Vec<u8> {
        BUFFER.with(|buf| {
            let mut buf = buf.borrow_mut();
            buf.clear();

            // ... serialization to buf ...

            buf.clone()
        })
    }
}
```

### Minor Fixes

**Rename hostname:**
```rust
const HOSTNAME: &str = "hostname";  // was "container_id"
```

**Fix MESSAGE_TYPE in RESERVED_FIELDS:**
```rust
const RESERVED_FIELDS: [&str; 11] = [
    DATE, RUNTIME, APPLICATION, LEVEL, HOSTNAME,
    MESSAGE, LOGGER, LINENO, FILE, VERSION, MESSAGE_TYPE,
];
```

**Remove lazy_static (tests/e2e.rs):**
```rust
static BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());
```

## Dependency Changes

**Cargo.toml:**

```toml
[package]
name = "struct-log"
version = "0.6.0"  # bump minor due to breaking change

[dependencies]
tracing-core = "0.1"
tracing-log = "0.2"
tracing-appender = "0.2"
# tracing-bunyan-formatter = "0.3"  # REMOVED
tracing = { version = "0.1", default-features = false, features = ["log", "std"] }
tracing-subscriber = { version = "0.3", default-features = false, features = ["registry", "fmt", "env-filter"] }

log = "0.4"
time = { version = "0.3", default-features = false, features = ["formatting"] }
gethostname = "1"

serde = "1.0"
serde_json = "1.0"

[dev-dependencies]
# lazy_static = "1.4"  # REMOVED
tracing = { version = "0.1", default-features = false, features = ["log", "std", "attributes"] }
time = { version = "0.3", default-features = false, features = ["formatting", "parsing", "local-offset"] }
```

**Removed dependencies:**
- `tracing-bunyan-formatter` (runtime)
- `lazy_static` (dev)

## Breaking Changes

1. `setup_logger` return type changes from `Option<WorkerGuard>` to `Result<Option<WorkerGuard>, SetupError>`
2. JSON field `container_id` renamed to `hostname`

## Usage Examples

**Simple (backward compatible with code change):**
```rust
let _guard = setup_logger("my-app".into(), "1.0.0".into())?;
```

**With builder:**
```rust
let _guard = StructLogBuilder::new("my-app", "1.0.0")
    .hostname("custom-host")
    .init()?;
```
