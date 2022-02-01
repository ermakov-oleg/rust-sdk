# struct-log

`struct-log` Provides JsonLogLayer (Layer implementation to be used on top of tracing Subscriber), which formats logs
into a structured json format

For local development, the default formatter `tracing_subscriber::fmt::init()` is used.

To enable the json formatter, you need to set the environment variable `JSON_LOG=true`.

### Example init logger

```rust
pub const APPLICATION_NAME: &str = "rust-example";
pub const VERSION: &str = "dev";


async fn main() -> Result<(), ()> {
    let _guard = setup_logger(APPLICATION_NAME.to_string(), VERSION.to_string());
}
```

### Example json log

```json
{
  "runtime": "rust",
  "application": "rust-example",
  "version": "dev",
  "container_id": "host.local",
  "date": "2022-02-01T19:31:39.417407Z",
  "level": "error",
  "logger": "runtime_settings::providers::file",
  "lineno": 34,
  "file": "lib/runtime-settings/src/providers/file.rs",
  "message": "Error: Could not update settings from file: \"/tmp/settings.json\"",
  "error": "Os { code: 2, kind: NotFound, message: \"No such file or directory\" }"
}
```