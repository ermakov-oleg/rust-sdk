# runtime-settings

Runtime configuration library for Rust microservices with context-aware settings filtering.

## Features

- **Multiple providers**: Environment variables, JSON files, MCS (Microservice Configuration Service)
- **12 filter types**: Static filters (application, server, environment, mcs_run_env, library_version) and dynamic filters (url-path, host, email, ip, header, context, probability)
- **Priority-based override**: Higher priority settings override lower ones
- **Vault integration**: Secret resolution via HashiCorp Vault
- **Change watchers**: Get notified when settings change
- **Scoped contexts**: Thread-local and task-local context storage with RAII guards

## Quick Start

```rust
use runtime_settings::{RuntimeSettings, Context, set_thread_context};

#[tokio::main]
async fn main() {
    // Build and initialize settings
    let settings = RuntimeSettings::builder()
        .application("my-service")
        .server("web-01")
        .mcs_enabled(true)
        .file_path("settings.json")
        .build();

    settings.init().await.expect("Failed to initialize settings");

    // Set context for current thread
    let ctx = Context {
        application: "my-service".to_string(),
        ..Default::default()
    };
    let _guard = settings.set_context(ctx);

    // Get settings (uses current context automatically)
    let value: Option<String> = settings.get("MY_SETTING");
    let with_default: String = settings.get_or("MY_SETTING", "default".to_string());
}
```

## Global Setup

For simpler usage with a global singleton:

```rust
use runtime_settings::{setup, settings, RuntimeSettings, set_thread_context, Context};

#[tokio::main]
async fn main() {
    // Initialize global settings
    setup(
        RuntimeSettings::builder()
            .application("my-service")
            .mcs_enabled(true)
    ).await.expect("Failed to setup");

    // Use anywhere in your code
    let ctx = Context {
        application: "my-service".to_string(),
        ..Default::default()
    };
    let _guard = settings().set_context(ctx);

    let value: Option<String> = settings().get("KEY");
}
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `RUNTIME_SETTINGS_APPLICATION` | Application name for `setup_from_env()` | `"unknown"` |
| `RUNTIME_SETTINGS_BASE_URL` | MCS service URL | `http://master.runtime-settings.dev3.cian.ru` |
| `RUNTIME_SETTINGS_FILE_PATH` | Path to settings JSON file | `settings.json` |
| `MCS_RUN_ENV` | MCS environment filter | None |
| `VAULT_ADDR` | Vault server address | Required for secrets |
| `VAULT_TOKEN` | Vault authentication token | Required for secrets |

## Settings File Format

Settings are stored as a JSON array. JSON5 format with comments is supported:

```json5
[
  {
    // Simple setting with no filters
    "key": "DATABASE_URL",
    "value": "postgres://localhost/mydb",
    "priority": 100
  },
  {
    // Setting with filters
    "key": "FEATURE_FLAG",
    "value": true,
    "priority": 200,
    "filter": {
      "application": "my-service",
      "email": "@example\\.com$"
    }
  },
  {
    // Setting with secret reference
    "key": "API_SECRET",
    "value": {"$secret": "secret/api:key"},
    "priority": 100
  }
]
```

## Filter Types

### Static Filters (checked once at load time)

| Filter | Description | Example |
|--------|-------------|---------|
| `application` | Exact match on application name | `"my-service"` |
| `server` | Exact match on server name | `"web-01"` |
| `environment` | Match environment variable | `"NODE_ENV=production"` |
| `mcs_run_env` | MCS environment filter | `"PROD"` |
| `library_version` | Semver version constraint | `">=1.0.0"` |

### Dynamic Filters (checked on each get())

| Filter | Description | Example |
|--------|-------------|---------|
| `url-path` | Regex match on request URL path | `"^/api/v2"` |
| `host` | Regex match on request host | `".*\\.example\\.com$"` |
| `email` | Regex match on user email | `"@admin\\.com$"` |
| `ip` | Regex match on client IP | `"^192\\.168\\."` |
| `header` | Match request header value | `"X-Feature:enabled"` |
| `context` | Match custom context values | `"tenant=acme;role=admin"` |
| `probability` | Random percentage (0-100) | `"50"` |

## Watchers

Monitor settings changes:

```rust
let watcher_id = settings.add_watcher("MY_KEY", Box::new(|old, new| {
    println!("MY_KEY changed from {:?} to {:?}", old, new);
}));

// Later, remove the watcher
settings.remove_watcher(watcher_id);
```

## Scoped Contexts

### Thread-Local Context

```rust
use runtime_settings::{set_thread_context, set_thread_request, Context, Request};

// Set context for current thread (restored on guard drop)
let ctx = Context { application: "app".to_string(), ..Default::default() };
let _guard = set_thread_context(ctx);

// Set request context
let req = Request { method: "GET".to_string(), path: "/api".to_string(), headers: Default::default() };
let _guard = set_thread_request(req);
```

### Task-Local Context (async)

```rust
use runtime_settings::{with_task_context, with_task_request, Context};

let ctx = Context { application: "app".to_string(), ..Default::default() };

with_task_context(ctx, async {
    // Context available within this async block
    let value: Option<String> = settings.get("KEY");
}).await;
```

## Providers

### Priority Order

1. **McsProvider** (priority: 0) - Settings from MCS service
2. **FileProvider** (priority: 1,000,000,000,000,000,000) - Local JSON file
3. **EnvProvider** (priority: -1,000,000,000,000,000,000) - Environment variables

Higher priority settings override lower ones when context matches.

### Custom Providers

Implement the `SettingsProvider` trait:

```rust
use runtime_settings::{SettingsProvider, ProviderResponse, SettingsError};
use async_trait::async_trait;

struct MyProvider;

#[async_trait]
impl SettingsProvider for MyProvider {
    async fn load(&self, current_version: &str) -> Result<ProviderResponse, SettingsError> {
        // Load settings from your source
        Ok(ProviderResponse {
            settings: vec![],
            deleted: vec![],
            version: "1".to_string(),
        })
    }

    fn default_priority(&self) -> i64 {
        0
    }

    fn name(&self) -> &'static str {
        "my-provider"
    }
}
```

## Vault Secrets

Settings can reference Vault secrets using the `$secret` pattern:

```json
{
  "key": "DATABASE_PASSWORD",
  "value": {"$secret": "secret/database:password"}
}
```

The format is `path:key` where:
- `path` is the Vault secret path
- `key` is the key within the secret data

Enable Vault in the builder:

```rust
RuntimeSettings::builder()
    .application("my-service")
    .vault_enabled(true)  // Uses VAULT_ADDR and VAULT_TOKEN env vars
    .build();
```
