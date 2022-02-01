# runtime-settings

Library for runtime configuration of applications from an external configuration source.

```rust
fn get_context() -> Context {
    Context {
        application: "application-name".to_string(),
        server: "test-server".into(),
        environment: HashMap::from([("TEST".to_string(), "foo".to_string())]),
        host: Some("hostname"),
        // Request context params
        url: None,
        url_path: None,
        email: None,
        ip: None,
        // Any other context
        context: Default::default(),
    }
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    runtime_settings::setup().await;

    let value: Option<String> = runtime_settings::settings.get(&key, &get_context());
}

```

## Implemented providers

### File provider

Reads the configuration file when the application starts.

Example configuration file:

```json
[
  {
    "runtime": "rust",
    "key": "SOME_KEY",
    "priority": 0,
    "filter": {
      "context": "foo=bar;baz=quix"
    },
    "value": "value"
  }
]
```

### Http provider

Description TBD