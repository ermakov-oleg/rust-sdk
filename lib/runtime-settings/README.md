# runtime-settings

Runtime configuration library for Rust microservices with context-aware settings filtering, multiple providers, and HashiCorp Vault integration.

## Features

- **Multiple providers**: Load settings from environment variables, JSON files, or MCS (Microservice Configuration Service)
- **12 filter types**: 5 static filters (checked at load time) and 7 dynamic filters (checked per request)
- **Priority-based override**: Higher priority settings override lower ones when filters match
- **Vault integration**: Lazy-loaded secrets from HashiCorp Vault with automatic refresh
- **Change watchers**: Get notified when settings change
- **Scoped contexts**: Thread-local and task-local context storage with RAII guards
- **High performance**: Pre-compiled filters, type-based caching, efficient lookups

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
runtime-settings = { path = "../lib/runtime-settings" }
```

## Quick Start

### Global Setup (Recommended)

```rust
use runtime_settings::{setup, settings, RuntimeSettings, Request};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Initialize global settings (note: .build() is called internally by setup())
    setup(
        RuntimeSettings::builder()
            .application("my-service")
            .mcs_enabled(true)
            .file_path("settings.json")
    ).await.expect("Failed to setup settings");

    // Set request context for the current thread
    let req = Request {
        method: "GET".to_string(),
        path: "/api/users".to_string(),
        headers: HashMap::new(),
    };
    let _guard = settings().set_request(req);

    // Get settings (uses current context automatically)
    let feature_enabled: Arc<bool> = settings().get_or("FEATURE_FLAG", false);
    let api_url: Option<Arc<String>> = settings().get("API_URL");

    // Access the value
    if *feature_enabled {
        println!("Feature is enabled!");
    }
}
```

### Instance-Based Usage

```rust
use runtime_settings::{RuntimeSettings, Request};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let settings = Arc::new(
        RuntimeSettings::builder()
            .application("my-service")
            .server("web-01")
            .mcs_enabled(true)
            .file_path("settings.json")
            .build()
    );

    settings.init().await.expect("Failed to initialize");

    // Use settings anywhere
    let value: Option<Arc<String>> = settings.get("MY_SETTING");
}
```

## Configuration

### Builder Options

| Method | Description | Default |
|--------|-------------|---------|
| `application(name)` | Application name for filtering | Empty string |
| `server(name)` | Server/hostname for filtering | System hostname |
| `library_version(name, version)` | Register library version for filtering | None |
| `mcs_enabled(bool)` | Enable MCS provider | `true` |
| `mcs_base_url(url)` | MCS service URL | From env or default |
| `file_path(path)` | Path to JSON settings file | None |
| `env_enabled(bool)` | Enable environment variable provider | `true` |
| `refresh_interval(duration)` | Background refresh interval | 30 seconds |

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `RUNTIME_SETTINGS_APPLICATION` | Application name for `setup_from_env()` | `"unknown"` |
| `RUNTIME_SETTINGS_BASE_URL` | MCS service URL | Required when MCS enabled |
| `RUNTIME_SETTINGS_FILE_PATH` | Path to settings JSON file | `runtime-settings.json` |
| `MCS_RUN_ENV` | MCS environment filter value | None |
| `VAULT_ADDR` | Vault server address | Required for secrets |
| `VAULT_TOKEN` | Vault authentication token | Required for secrets |
| `STATIC_SECRETS_REFRESH_INTERVALS` | JSON map of path patterns to refresh intervals | See Secrets section |

### Settings File Format

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
    // Setting with filters - applies only to specific app and URL pattern
    "key": "FEATURE_FLAG",
    "value": true,
    "priority": 200,
    "filter": {
      "application": "my-service",
      "url-path": "^/api/v2/.*"
    }
  },
  {
    // Setting with secret reference
    "key": "API_SECRET",
    "value": {"$secret": "secret/api:key"},
    "priority": 100
  },
  {
    // Complex value with nested secret
    "key": "DB_CONFIG",
    "value": {
      "host": "localhost",
      "port": 5432,
      "password": {"$secret": "database/prod:password"}
    },
    "priority": 100
  }
]
```

## Core Concepts

### Providers

Settings are loaded from multiple providers, each with a default priority:

| Provider | Priority | Description |
|----------|----------|-------------|
| **FileProvider** | 10^18 (highest) | Local JSON/JSON5 file for overrides |
| **McsProvider** | 0 (default) | Remote configuration service (settings include their own priority) |
| **EnvProvider** | -10^18 (lowest) | Environment variables as fallback |

**Loading order**: EnvProvider → FileProvider → McsProvider

Higher priority settings override lower ones when multiple settings match the same key.

### Static vs Dynamic Filters

**Static filters** are checked once when settings are loaded. Settings that don't match static filters are discarded immediately, reducing memory usage.

**Dynamic filters** are checked on every `get()` call. This allows per-request filtering based on the current context.

### Priority System

When multiple settings exist for the same key:

1. Settings are sorted by priority (highest first)
2. On `get()`, the first setting matching all dynamic filters is returned
3. If no setting matches, `None` is returned

### Context Types

```rust
// StaticContext - set once at initialization
pub struct StaticContext {
    pub application: String,           // App name
    pub server: String,                 // Hostname
    pub environment: HashMap<String, String>, // All env vars
    pub libraries_versions: HashMap<String, Version>,
    pub mcs_run_env: Option<String>,
}

// DynamicContext - changes per request
pub struct DynamicContext {
    pub request: Option<Request>,
    pub custom: CustomContext,
}

// Request - HTTP request information
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}
```

## Usage Patterns

### Thread-Local Context

For synchronous code, use RAII guards:

```rust
use runtime_settings::{settings, Request};
use std::collections::HashMap;
use std::sync::Arc;

fn handle_request() {
    // Set request context (restored on guard drop)
    let req = Request {
        method: "GET".to_string(),
        path: "/api/users".to_string(),
        headers: [("host".to_string(), "api.example.com".to_string())].into(),
    };
    let _request_guard = settings().set_request(req);

    // Add custom context layer
    let custom: HashMap<String, String> = [
        ("user_id".to_string(), "123".to_string()),
        ("role".to_string(), "admin".to_string()),
    ].into();
    let _custom_guard = settings().set_custom(custom);

    // Settings now use both request and custom context
    let value: Option<Arc<String>> = settings().get("ADMIN_FEATURE");

    // Guards dropped here, context restored
}
```

### Task-Local Context (Async)

For async code, use the `with_*` methods:

```rust
use runtime_settings::{settings, Request};
use std::sync::Arc;

async fn handle_async_request() {
    let req = Request {
        method: "POST".to_string(),
        path: "/api/orders".to_string(),
        headers: Default::default(),
    };

    settings().with_request(req, async {
        // Request context available within this async block
        let value: Option<Arc<String>> = settings().get("ORDER_FEATURE");

        // Can nest custom context
        let custom = [("tenant".to_string(), "acme".to_string())].into();
        settings().with_custom(custom, async {
            let tenant_value: Option<Arc<bool>> = settings().get("TENANT_FEATURE");
        }).await;
    }).await;
}
```

### Standalone Context Functions

The context functions are also available as standalone imports:

```rust
use runtime_settings::{
    set_thread_request, set_thread_custom,
    with_task_request, with_task_custom,
    current_request, current_custom,
    Request, CustomContextGuard, RequestGuard,
};
use std::collections::HashMap;

// These work the same as settings().set_request(), etc.
let req = Request { method: "GET".into(), path: "/".into(), headers: HashMap::new() };
let _guard: RequestGuard = set_thread_request(req);

// Check current context
if let Some(req) = current_request() {
    println!("Current path: {}", req.path);
}
```

### Combining Contexts

Custom context supports hierarchical layering:

```rust
let outer: HashMap<String, String> = [("a".to_string(), "1".to_string())].into();
let _guard1 = settings().set_custom(outer);
// custom = {"a": "1"}

let inner: HashMap<String, String> = [
    ("b".to_string(), "2".to_string()),
    ("a".to_string(), "override".to_string()),
].into();
let _guard2 = settings().set_custom(inner);
// custom = {"a": "override", "b": "2"}

// When _guard2 drops: custom = {"a": "1"}
// When _guard1 drops: custom = {}
```

## Filters Reference

### Static Filters

Checked once at load time. Settings that don't match are discarded.

| Filter | Pattern | Matches Against | Example |
|--------|---------|-----------------|---------|
| `application` | Regex | `StaticContext.application` | `"my-service"`, `"my-.*"` |
| `server` | Regex | `StaticContext.server` | `"prod-web-.*"` |
| `mcs_run_env` | Regex | `StaticContext.mcs_run_env` | `"PROD"`, `"DEV\|STAGE"` |
| `environment` | `KEY=regex,KEY2=regex` | Environment variables | `"ENV=prod,DEBUG=false"` |
| `library_version` | `pkg>=1.0.0,pkg<2.0` | Registered library versions | `"my-lib>=1.0.0"` |

### Dynamic Filters

Checked on every `get()` call. Return `true` (pass) when context is not available.

| Filter | Pattern | Matches Against | Example |
|--------|---------|-----------------|---------|
| `url-path` | Regex | `Request.path` | `"^/api/v2/.*"` |
| `host` | Regex | `Request.headers["host"]` | `".*\\.example\\.com$"` |
| `email` | Regex | `Request.headers["x-real-email"]` | `".*@admin\\.com$"` |
| `ip` | Regex | `Request.headers["x-real-ip"]` | `"^192\\.168\\..*"` |
| `header` | `Key=regex,Key2=regex` | Request headers (case-insensitive) | `"X-Feature=enabled"` |
| `context` | `key=regex,key2=regex` | Custom context values | `"tenant=acme"` |
| `probability` | `0-100` | Random percentage | `"25"` (25% chance) |

### Filter Pattern Rules

- All regex patterns are **case-insensitive**
- Patterns are **automatically anchored** (`^pattern$`)
- Multiple conditions in `environment`, `header`, `context` use AND logic
- Unknown filters are silently ignored (backwards compatibility)

### Filter Examples

```json5
[
  // Only for production environment
  {
    "key": "CACHE_TTL",
    "value": 3600,
    "filter": {"environment": "ENV=production"}
  },

  // Only for specific API endpoints
  {
    "key": "RATE_LIMIT",
    "value": 100,
    "filter": {"url-path": "^/api/v2/.*"}
  },

  // Gradual rollout to 10% of requests
  {
    "key": "NEW_ALGORITHM",
    "value": true,
    "filter": {"probability": "10"}
  },

  // Admin users only
  {
    "key": "ADMIN_PANEL",
    "value": true,
    "filter": {"email": ".*@admin\\.example\\.com$"}
  },

  // Specific tenant in custom context
  {
    "key": "CUSTOM_BRANDING",
    "value": true,
    "filter": {"context": "tenant=acme"}
  },

  // Combined filters (all must match)
  {
    "key": "BETA_FEATURE",
    "value": true,
    "filter": {
      "application": "my-service",
      "environment": "ENV=staging",
      "probability": "50"
    }
  }
]
```

## Vault Secrets Integration

### Configuration

Enable Vault by setting environment variables:

```bash
export VAULT_ADDR="https://vault.example.com"
export VAULT_TOKEN="s.xxxxx"
```

Vault is automatically configured when these variables are present. If not configured, settings without secrets work normally, but secret references will fail.

### Secret Reference Syntax

Use `{"$secret": "path:key"}` in setting values:

```json
{
  "key": "DB_PASSWORD",
  "value": {"$secret": "database/prod:password"}
}
```

Format: `vault_path:key_in_secret`

- `path`: Vault KV v2 path (e.g., `database/prod`)
- `key`: Key within the secret data (e.g., `password`)

Secrets can be nested in complex values:

```json
{
  "key": "DATABASE_CONFIG",
  "value": {
    "host": "db.example.com",
    "port": 5432,
    "credentials": {
      "username": "app",
      "password": {"$secret": "database/prod:password"}
    }
  }
}
```

### Lazy Loading Behavior

Secrets are **not** fetched at initialization. Instead:

1. When a setting with secrets is first accessed via `get()`, secrets are resolved
2. First access is slow (Vault network call)
3. Subsequent accesses return cached values instantly
4. Cache is invalidated when secrets are refreshed in background

This design allows different application instances to access only the secrets they need.

### Secret Refresh

Secrets are refreshed during `RuntimeSettings::refresh()` (called automatically every 30s by default):

**Renewable secrets** (with Vault lease): Refreshed when 75% of lease duration has elapsed.

**Static secrets**: Refreshed based on configurable intervals:

```bash
# Default intervals (in seconds)
# - kafka-certificates: 600
# - interservice-auth: 60

# Override via environment variable
export STATIC_SECRETS_REFRESH_INTERVALS='{"my-secret-path": 300}'
```

### Cache Invalidation

The library tracks a global secrets version. When any secret value changes during refresh:

1. Global version is incremented
2. Settings with secrets check their cached version
3. Stale caches are cleared, forcing re-resolution on next access

## Watchers

Monitor settings changes and react to them:

```rust
use runtime_settings::settings;

// Add a watcher
let watcher_id = settings().add_watcher("FEATURE_FLAG", Box::new(|old, new| {
    Box::pin(async move {
        println!("FEATURE_FLAG changed from {:?} to {:?}", old, new);
        // Perform any async cleanup or reinitialization
    })
}));

// Remove when no longer needed
settings().remove_watcher(watcher_id);
```

### Watcher Behavior

- Watchers are checked during `refresh()` after settings and secrets are updated
- Multiple watchers can be registered for the same key
- Watcher panics are caught and logged (one panic doesn't affect others)
- Callbacks receive `Option<serde_json::Value>` for old and new values

### Use Cases

- Invalidate application caches when settings change
- Log configuration changes for audit
- Trigger reconnection when connection strings change
- Update in-memory feature flag state

## Advanced Topics

### Getter Functions

Create reusable getter functions with defaults:

```rust
use runtime_settings::{settings, RuntimeSettings};
use std::sync::Arc;

// Create a getter function
let get_timeout = settings().getter("REQUEST_TIMEOUT_MS", 5000u64);

// Use it anywhere
let timeout: Arc<u64> = get_timeout(settings());
println!("Timeout: {} ms", *timeout);
```

### Refresh with Timeout

Prevent refresh operations from blocking indefinitely:

```rust
use runtime_settings::settings;
use std::time::Duration;

// Refresh with 5-second timeout
match settings().refresh_with_timeout(Duration::from_secs(5)).await {
    Ok(()) => println!("Settings refreshed"),
    Err(e) => eprintln!("Refresh failed or timed out: {}", e),
}
```

### Custom Providers

Implement the `SettingsProvider` trait:

```rust
use runtime_settings::{SettingsProvider, ProviderResponse, SettingsError, RawSetting};
use async_trait::async_trait;

struct MyProvider {
    source_url: String,
}

#[async_trait]
impl SettingsProvider for MyProvider {
    async fn load(&self, current_version: &str) -> Result<ProviderResponse, SettingsError> {
        // Fetch settings from your source
        // current_version can be used for incremental updates

        Ok(ProviderResponse {
            settings: vec![
                RawSetting {
                    key: "MY_KEY".to_string(),
                    priority: 500,
                    filter: Default::default(),
                    value: serde_json::json!("my_value"),
                }
            ],
            deleted: vec![],  // Keys to remove (for incremental updates)
            version: "1".to_string(),
        })
    }

    fn default_priority(&self) -> i64 {
        500  // Between Env (-10^18) and File (10^18)
    }

    fn name(&self) -> &'static str {
        "my-provider"
    }
}
```

### Performance Considerations

**Filter Compilation**: All regex patterns are compiled once when settings are loaded, not on every check.

**Type Caching**: Deserialized values are cached by `TypeId`. First access deserializes, subsequent accesses return `Arc` clones.

```rust
// First call: deserializes JSON to MyConfig, stores Arc<MyConfig>
let config1: Arc<MyConfig> = settings().get("CONFIG").unwrap();

// Second call: returns cloned Arc (no deserialization)
let config2: Arc<MyConfig> = settings().get("CONFIG").unwrap();

// Different type: deserializes again, caches separately
let config_str: Arc<String> = settings().get("CONFIG").unwrap();
```

**Static Pre-filtering**: Settings not matching static filters are discarded at load time, reducing memory and lookup time.

**CustomContext Snapshots**: Each layer creates a merged snapshot for O(1) lookups instead of traversing a stack.

### Multi-threaded Runtime Requirement

**Important**: Synchronous secret resolution uses `tokio::task::block_in_place()`, which requires a **multi-threaded Tokio runtime**.

```rust
// Correct: multi-threaded runtime
#[tokio::main]
async fn main() { /* ... */ }

// Or explicitly:
#[tokio::main(flavor = "multi_thread")]
async fn main() { /* ... */ }

// Will panic with secrets:
#[tokio::main(flavor = "current_thread")]
async fn main() { /* ... */ }
```

### Error Handling

The library uses `SettingsError` for all error conditions:

| Error | When It Occurs |
|-------|---------------|
| `FileRead` | Cannot read settings file |
| `JsonParse` | Invalid JSON/JSON5 in settings file |
| `McsRequest` | Network error when contacting MCS |
| `McsResponse` | MCS returned an error status |
| `SecretNotFound` | Vault secret path doesn't exist |
| `SecretKeyNotFound` | Key not found within Vault secret |
| `InvalidSecretReference` | Malformed `$secret` syntax (missing `:`) |
| `SecretWithoutVault` | Secret used but `VAULT_ADDR`/`VAULT_TOKEN` not set |
| `Vault` | General Vault communication error |
| `InvalidRegex` | Invalid regex pattern in filter |
| `InvalidVersionSpec` | Invalid version constraint in `library_version` filter |
| `Timeout` | Operation timed out (from `refresh_with_timeout`) |

### Troubleshooting

**Settings not found**:
- Check if static filters match your `StaticContext`
- Verify priority order (higher priority wins)
- Enable debug logging: `RUST_LOG=runtime_settings=debug`

**Secrets failing**:
- Verify `VAULT_ADDR` and `VAULT_TOKEN` are set
- Check Vault permissions for the secret path
- Ensure multi-threaded Tokio runtime

**Watchers not firing**:
- Watchers only fire during `refresh()`, not on initial load
- Check if the setting value actually changed (JSON comparison)

**Memory usage**:
- Settings not matching static filters are discarded
- Each unique type requested creates a cached entry
- Consider using `Arc<T>` for large settings to share memory

## Architecture (for Contributors)

### Data Flow

```
Providers (File, MCS, Env)
         │
         ▼
   RawSetting (JSON)
         │
         ▼
  Setting::compile()
    ├── Compile static filters (regex)
    ├── Compile dynamic filters (regex)
    └── Parse secret references
         │
         ▼
   SettingsState
    └── HashMap<key, Vec<Setting>> (sorted by priority)
         │
         ▼
  RuntimeSettings::get()
    ├── Lookup by key
    ├── Check dynamic filters
    ├── Invalidate stale secrets cache
    ├── Resolve secrets (if needed)
    ├── Deserialize to type T
    └── Cache by TypeId
```

### Key Internal Structures

```rust
// Settings storage
struct SettingsState {
    version: String,  // MCS version for incremental updates
    settings: HashMap<String, Vec<Setting>>,  // Key -> priority-sorted list
}

// Compiled setting
pub struct Setting {
    pub key: String,
    pub priority: i64,
    pub value: serde_json::Value,
    pub static_filters: Vec<Box<dyn CompiledStaticFilter>>,
    pub dynamic_filters: Vec<Box<dyn CompiledDynamicFilter>>,
    value_cache: DashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    secrets_usages: Vec<SecretUsage>,
    cached_at_version: AtomicU64,
}

// Secrets cache
pub struct SecretsService {
    client: Option<VaultClient>,
    cache: RwLock<HashMap<String, CachedSecret>>,
    refresh_intervals: HashMap<String, Duration>,
    version: AtomicU64,  // Incremented on any secret change
}
```

### Caching Strategy

1. **Secrets Cache** (`SecretsService.cache`): Path → full secret data. Shared across all settings.

2. **Type Cache** (`Setting.value_cache`): TypeId → Arc<T>. Per-setting, per-type.

3. **Version Tracking**: Global `SecretsService.version` vs per-setting `cached_at_version`. Triggers cache clear on mismatch.

### Filter Compilation

Filters are compiled into trait objects for efficient dispatch:

```rust
pub trait CompiledStaticFilter: Send + Sync {
    fn check(&self, ctx: &StaticContext) -> bool;
}

pub trait CompiledDynamicFilter: Send + Sync {
    fn check(&self, ctx: &DynamicContext) -> bool;
}
```

Unknown filters return `Ok` from compilation factory, are silently ignored (backwards compatibility).

### Thread Safety

- `SettingsState`: Protected by `RwLock` (read-heavy workload)
- `Setting.value_cache`: `DashMap` for concurrent type caching
- `SecretsService.cache`: `RwLock` with separate read/write paths
- Atomic version counters for lock-free version checks

### Module Structure

```
src/
├── lib.rs          # Public exports
├── settings.rs     # RuntimeSettings, Builder
├── entities.rs     # RawSetting, Setting, compilation
├── context.rs      # StaticContext, DynamicContext, Request, CustomContext
├── scoped.rs       # Thread-local and task-local storage
├── setup.rs        # Global singleton, background refresh
├── error.rs        # SettingsError enum
├── watchers.rs     # WatchersService
├── providers/
│   ├── mod.rs      # SettingsProvider trait
│   ├── file.rs     # FileProvider
│   ├── mcs.rs      # McsProvider
│   └── env.rs      # EnvProvider
├── filters/
│   ├── mod.rs      # Filter traits, compilation
│   ├── static_filters.rs
│   └── dynamic_filters.rs
└── secrets/
    ├── mod.rs      # SecretsService
    └── resolver.rs # Sync/async resolution
```

## License

MIT
