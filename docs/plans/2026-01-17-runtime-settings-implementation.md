# Runtime Settings v2 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rewrite runtime-settings library with full Python cian-settings parity (except Consul and FakeSettings).

**Architecture:** Provider-based settings loading (env, file, MCS) with 12-filter system (static + dynamic), Vault integration for secrets, watchers for change notifications, and thread-local/task-local scoped contexts.

**Tech Stack:** Rust, tokio, reqwest, vaultrs, serde, regex, semver

**Working directory:** `/Users/o.ermakov/projects/rust-sdk/.worktrees/runtime-settings`

---

## Phase 1: Core Infrastructure

### Task 1.1: Clean up existing code and setup new structure

**Files:**
- Delete all files in: `lib/runtime-settings/src/`
- Create: `lib/runtime-settings/src/lib.rs`
- Create: `lib/runtime-settings/src/error.rs`

**Step 1: Remove old implementation**

```bash
rm -rf lib/runtime-settings/src/*.rs lib/runtime-settings/src/providers/
```

**Step 2: Create new lib.rs skeleton**

```rust
// lib/runtime-settings/src/lib.rs
pub mod error;

pub use error::SettingsError;
```

**Step 3: Create error module**

```rust
// lib/runtime-settings/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("Failed to read settings file: {0}")]
    FileRead(#[from] std::io::Error),

    #[error("Failed to parse settings JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("MCS request failed: {0}")]
    McsRequest(#[from] reqwest::Error),

    #[error("MCS returned error: status={status}, message={message}")]
    McsResponse { status: u16, message: String },

    #[error("Secret not found: {path}")]
    SecretNotFound { path: String },

    #[error("Secret key not found: {key} in {path}")]
    SecretKeyNotFound { path: String, key: String },

    #[error("Invalid secret reference: {reference}")]
    InvalidSecretReference { reference: String },

    #[error("Secret used but Vault not configured")]
    SecretWithoutVault,

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Invalid regex pattern: {pattern}, error: {error}")]
    InvalidRegex { pattern: String, error: String },

    #[error("Invalid version specifier: {spec}")]
    InvalidVersionSpec { spec: String },

    #[error("Context not set - call set_context() or with_context() first")]
    ContextNotSet,
}
```

**Step 4: Update Cargo.toml dependencies**

```toml
# lib/runtime-settings/Cargo.toml
[package]
name = "runtime-settings"
version = "0.2.0"
edition = "2021"

[dependencies]
# Async runtime
tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }
async-trait = "0.1"
futures = "0.3"

# HTTP
reqwest = { version = "0.11", features = ["json"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
json5 = "0.4"

# Vault
vaultrs = "0.7"

# Utilities
thiserror = "2"
tracing = "0.1"
lazy_static = "1.4"
regex = "1"
semver = "1"
rand = "0.8"

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

**Step 5: Verify it compiles**

Run: `cargo check -p runtime-settings`
Expected: Success (warnings OK)

**Step 6: Commit**

```bash
git add -A && git commit -m "Reset runtime-settings for v2 rewrite"
```

---

### Task 1.2: Implement entities module

**Files:**
- Create: `lib/runtime-settings/src/entities.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Write entities tests**

```rust
// lib/runtime-settings/src/entities.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setting_deserialize() {
        let json = r#"{
            "key": "MY_KEY",
            "priority": 100,
            "filter": {"application": "my-app"},
            "value": "test-value"
        }"#;
        let setting: Setting = serde_json::from_str(json).unwrap();
        assert_eq!(setting.key, "MY_KEY");
        assert_eq!(setting.priority, 100);
        assert_eq!(setting.filter.get("application"), Some(&"my-app".to_string()));
    }

    #[test]
    fn test_setting_deserialize_without_filter() {
        let json = r#"{"key": "KEY", "priority": 0, "value": 123}"#;
        let setting: Setting = serde_json::from_str(json).unwrap();
        assert!(setting.filter.is_empty());
        assert_eq!(setting.value, serde_json::json!(123));
    }

    #[test]
    fn test_setting_key_deserialize() {
        let json = r#"{"key": "KEY", "priority": -1000000000000000000}"#;
        let key: SettingKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.key, "KEY");
        assert_eq!(key.priority, -1_000_000_000_000_000_000);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings`
Expected: FAIL (Setting not found)

**Step 3: Implement entities**

```rust
// lib/runtime-settings/src/entities.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One setting from any source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub priority: i64,
    #[serde(default)]
    pub filter: HashMap<String, String>,
    pub value: serde_json::Value,
}

/// Identifier for deleting a setting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingKey {
    pub key: String,
    pub priority: i64,
}

/// Response from MCS
#[derive(Debug, Clone, Deserialize)]
pub struct McsResponse {
    pub settings: Vec<Setting>,
    #[serde(default)]
    pub deleted: Vec<SettingKey>,
    pub version: String,
}

// ... tests at the bottom
```

**Step 4: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs
pub mod entities;
pub mod error;

pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p runtime-settings`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "Add entities module with Setting, SettingKey, McsResponse"
```

---

### Task 1.3: Implement context module

**Files:**
- Create: `lib/runtime-settings/src/context.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Write context tests**

```rust
// lib/runtime-settings/src/context.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_host_from_header() {
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), "example.com".to_string());
        let request = Request {
            method: "GET".to_string(),
            path: "/api".to_string(),
            headers,
        };
        assert_eq!(request.host(), Some("example.com"));
    }

    #[test]
    fn test_request_ip_from_header() {
        let mut headers = HashMap::new();
        headers.insert("x-real-ip".to_string(), "192.168.1.1".to_string());
        let request = Request {
            method: "GET".to_string(),
            path: "/".to_string(),
            headers,
        };
        assert_eq!(request.ip(), Some("192.168.1.1"));
    }

    #[test]
    fn test_request_email_from_header() {
        let mut headers = HashMap::new();
        headers.insert("x-real-email".to_string(), "user@example.com".to_string());
        let request = Request {
            method: "GET".to_string(),
            path: "/".to_string(),
            headers,
        };
        assert_eq!(request.email(), Some("user@example.com"));
    }

    #[test]
    fn test_request_headers_case_insensitive() {
        let mut headers = HashMap::new();
        headers.insert("X-Real-IP".to_string(), "10.0.0.1".to_string());
        let request = Request {
            method: "GET".to_string(),
            path: "/".to_string(),
            headers,
        };
        assert_eq!(request.ip(), Some("10.0.0.1"));
    }

    #[test]
    fn test_context_default() {
        let ctx = Context::default();
        assert!(ctx.application.is_empty());
        assert!(ctx.server.is_empty());
        assert!(ctx.request.is_none());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings context`
Expected: FAIL

**Step 3: Implement context**

```rust
// lib/runtime-settings/src/context.rs
use semver::Version;
use std::collections::HashMap;

/// HTTP request for context filtering
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}

impl Request {
    /// Get host from "host" header (case-insensitive)
    pub fn host(&self) -> Option<&str> {
        self.get_header("host")
    }

    /// Get IP from "x-real-ip" header (case-insensitive)
    pub fn ip(&self) -> Option<&str> {
        self.get_header("x-real-ip")
    }

    /// Get email from "x-real-email" header (case-insensitive)
    pub fn email(&self) -> Option<&str> {
        self.get_header("x-real-email")
    }

    /// Get header value (case-insensitive key lookup)
    pub fn get_header(&self, key: &str) -> Option<&str> {
        let key_lower = key.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == key_lower)
            .map(|(_, v)| v.as_str())
    }
}

/// Full context for filtering
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub application: String,
    pub server: String,
    pub environment: HashMap<String, String>,
    pub libraries_versions: HashMap<String, Version>,
    pub mcs_run_env: Option<String>,
    pub request: Option<Request>,
    pub custom: HashMap<String, String>,
}

/// Static context (doesn't change after init)
#[derive(Debug, Clone)]
pub struct StaticContext {
    pub application: String,
    pub server: String,
    pub environment: HashMap<String, String>,
    pub libraries_versions: HashMap<String, Version>,
    pub mcs_run_env: Option<String>,
}

impl From<&Context> for StaticContext {
    fn from(ctx: &Context) -> Self {
        Self {
            application: ctx.application.clone(),
            server: ctx.server.clone(),
            environment: ctx.environment.clone(),
            libraries_versions: ctx.libraries_versions.clone(),
            mcs_run_env: ctx.mcs_run_env.clone(),
        }
    }
}

// ... tests at the bottom
```

**Step 4: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs
pub mod context;
pub mod entities;
pub mod error;

pub use context::{Context, Request, StaticContext};
pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
```

**Step 5: Run tests**

Run: `cargo test -p runtime-settings`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "Add context module with Request, Context, StaticContext"
```

---

## Phase 2: Filters

### Task 2.1: Create filters module structure

**Files:**
- Create: `lib/runtime-settings/src/filters/mod.rs`
- Create: `lib/runtime-settings/src/filters/static_filters.rs`
- Create: `lib/runtime-settings/src/filters/dynamic_filters.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Create filter traits and result enum**

```rust
// lib/runtime-settings/src/filters/mod.rs
pub mod dynamic_filters;
pub mod static_filters;

use crate::context::{Context, StaticContext};

/// Result of filter check
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterResult {
    Match,
    NoMatch,
    NotApplicable,
}

/// Static filter - checked once when loading settings
pub trait StaticFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult;
}

/// Dynamic filter - checked on every get()
pub trait DynamicFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult;
}

pub use dynamic_filters::*;
pub use static_filters::*;
```

**Step 2: Create static_filters.rs skeleton**

```rust
// lib/runtime-settings/src/filters/static_filters.rs
use super::{FilterResult, StaticFilter};
use crate::context::StaticContext;

pub struct ApplicationFilter;
pub struct ServerFilter;
pub struct EnvironmentFilter;
pub struct McsRunEnvFilter;
pub struct LibraryVersionFilter;
```

**Step 3: Create dynamic_filters.rs skeleton**

```rust
// lib/runtime-settings/src/filters/dynamic_filters.rs
use super::{DynamicFilter, FilterResult};
use crate::context::Context;

pub struct UrlPathFilter;
pub struct HostFilter;
pub struct EmailFilter;
pub struct IpFilter;
pub struct HeaderFilter;
pub struct ContextFilter;
pub struct ProbabilityFilter;
```

**Step 4: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs
pub mod context;
pub mod entities;
pub mod error;
pub mod filters;

pub use context::{Context, Request, StaticContext};
pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
pub use filters::FilterResult;
```

**Step 5: Verify it compiles**

Run: `cargo check -p runtime-settings`
Expected: Success

**Step 6: Commit**

```bash
git add -A && git commit -m "Add filters module structure"
```

---

### Task 2.2: Implement regex-based static filters (application, server, mcs_run_env)

**Files:**
- Modify: `lib/runtime-settings/src/filters/static_filters.rs`

**Step 1: Write tests for ApplicationFilter**

```rust
// Add to lib/runtime-settings/src/filters/static_filters.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_static_ctx(app: &str, server: &str) -> StaticContext {
        StaticContext {
            application: app.to_string(),
            server: server.to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        }
    }

    #[test]
    fn test_application_filter_match() {
        let filter = ApplicationFilter;
        let ctx = make_static_ctx("my-service", "server1");
        assert_eq!(filter.check("my-service", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_application_filter_regex_match() {
        let filter = ApplicationFilter;
        let ctx = make_static_ctx("my-service-prod", "server1");
        assert_eq!(filter.check("my-service-.*", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_application_filter_no_match() {
        let filter = ApplicationFilter;
        let ctx = make_static_ctx("other-service", "server1");
        assert_eq!(filter.check("my-service", &ctx), FilterResult::NoMatch);
    }

    #[test]
    fn test_server_filter_match() {
        let filter = ServerFilter;
        let ctx = make_static_ctx("app", "prod-server-1");
        assert_eq!(filter.check("prod-server-.*", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_mcs_run_env_filter_match() {
        let filter = McsRunEnvFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.mcs_run_env = Some("PROD".to_string());
        assert_eq!(filter.check("PROD", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_mcs_run_env_filter_not_applicable() {
        let filter = McsRunEnvFilter;
        let ctx = make_static_ctx("app", "server");
        assert_eq!(filter.check("PROD", &ctx), FilterResult::NotApplicable);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings filters::static_filters`
Expected: FAIL

**Step 3: Implement regex-based static filters**

```rust
// lib/runtime-settings/src/filters/static_filters.rs
use super::{FilterResult, StaticFilter};
use crate::context::StaticContext;
use regex::RegexBuilder;

/// Helper to check regex pattern against value (case-insensitive, anchored)
fn check_regex(pattern: &str, value: &str) -> FilterResult {
    let anchored = format!("^(?:{})$", pattern);
    match RegexBuilder::new(&anchored)
        .case_insensitive(true)
        .build()
    {
        Ok(re) => {
            if re.is_match(value) {
                FilterResult::Match
            } else {
                FilterResult::NoMatch
            }
        }
        Err(_) => FilterResult::NoMatch,
    }
}

/// application: regex against ctx.application
pub struct ApplicationFilter;

impl StaticFilter for ApplicationFilter {
    fn name(&self) -> &'static str {
        "application"
    }

    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult {
        check_regex(pattern, &ctx.application)
    }
}

/// server: regex against ctx.server
pub struct ServerFilter;

impl StaticFilter for ServerFilter {
    fn name(&self) -> &'static str {
        "server"
    }

    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult {
        check_regex(pattern, &ctx.server)
    }
}

/// mcs_run_env: regex against ctx.mcs_run_env
pub struct McsRunEnvFilter;

impl StaticFilter for McsRunEnvFilter {
    fn name(&self) -> &'static str {
        "mcs_run_env"
    }

    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult {
        match &ctx.mcs_run_env {
            Some(env) => check_regex(pattern, env),
            None => FilterResult::NotApplicable,
        }
    }
}

// ... tests at the bottom
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings filters::static_filters`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement ApplicationFilter, ServerFilter, McsRunEnvFilter"
```

---

### Task 2.3: Implement EnvironmentFilter (map-based)

**Files:**
- Modify: `lib/runtime-settings/src/filters/static_filters.rs`

**Step 1: Add tests**

```rust
// Add to static_filters.rs tests module
#[test]
fn test_environment_filter_single_match() {
    let filter = EnvironmentFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.environment.insert("ENV".to_string(), "prod".to_string());
    assert_eq!(filter.check("ENV=prod", &ctx), FilterResult::Match);
}

#[test]
fn test_environment_filter_multiple_match() {
    let filter = EnvironmentFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.environment.insert("ENV".to_string(), "prod".to_string());
    ctx.environment.insert("DEBUG".to_string(), "false".to_string());
    assert_eq!(filter.check("ENV=prod,DEBUG=false", &ctx), FilterResult::Match);
}

#[test]
fn test_environment_filter_partial_no_match() {
    let filter = EnvironmentFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.environment.insert("ENV".to_string(), "prod".to_string());
    // DEBUG is missing
    assert_eq!(filter.check("ENV=prod,DEBUG=false", &ctx), FilterResult::NoMatch);
}

#[test]
fn test_environment_filter_regex_value() {
    let filter = EnvironmentFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.environment.insert("ENV".to_string(), "production".to_string());
    assert_eq!(filter.check("ENV=prod.*", &ctx), FilterResult::Match);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings environment_filter`
Expected: FAIL

**Step 3: Implement EnvironmentFilter**

```rust
// Add to static_filters.rs after McsRunEnvFilter

/// Helper to parse "KEY1=value1,KEY2=value2" format and check against a map
fn check_map_filter(
    pattern: &str,
    map: &std::collections::HashMap<String, String>,
) -> FilterResult {
    for pair in pattern.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() != 2 {
            return FilterResult::NoMatch;
        }
        let key = parts[0].trim();
        let value_pattern = parts[1].trim();

        match map.get(key) {
            Some(actual_value) => {
                if check_regex(value_pattern, actual_value) != FilterResult::Match {
                    return FilterResult::NoMatch;
                }
            }
            None => return FilterResult::NoMatch,
        }
    }
    FilterResult::Match
}

/// environment: "KEY1=val1,KEY2=val2" against ctx.environment
pub struct EnvironmentFilter;

impl StaticFilter for EnvironmentFilter {
    fn name(&self) -> &'static str {
        "environment"
    }

    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult {
        check_map_filter(pattern, &ctx.environment)
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings environment_filter`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement EnvironmentFilter"
```

---

### Task 2.4: Implement LibraryVersionFilter (semver-based)

**Files:**
- Modify: `lib/runtime-settings/src/filters/static_filters.rs`

**Step 1: Add tests**

```rust
// Add to static_filters.rs tests module
#[test]
fn test_library_version_filter_exact_match() {
    let filter = LibraryVersionFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.libraries_versions.insert("my-lib".to_string(), semver::Version::new(1, 2, 3));
    assert_eq!(filter.check("my-lib=1.2.3", &ctx), FilterResult::Match);
}

#[test]
fn test_library_version_filter_gte() {
    let filter = LibraryVersionFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.libraries_versions.insert("my-lib".to_string(), semver::Version::new(2, 0, 0));
    assert_eq!(filter.check("my-lib>=1.0.0", &ctx), FilterResult::Match);
}

#[test]
fn test_library_version_filter_range() {
    let filter = LibraryVersionFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.libraries_versions.insert("my-lib".to_string(), semver::Version::new(1, 5, 0));
    assert_eq!(filter.check("my-lib>=1.0.0,my-lib<2.0.0", &ctx), FilterResult::Match);
}

#[test]
fn test_library_version_filter_not_installed() {
    let filter = LibraryVersionFilter;
    let ctx = make_static_ctx("app", "server");
    assert_eq!(filter.check("my-lib>=1.0.0", &ctx), FilterResult::NoMatch);
}

#[test]
fn test_library_version_filter_version_too_low() {
    let filter = LibraryVersionFilter;
    let mut ctx = make_static_ctx("app", "server");
    ctx.libraries_versions.insert("my-lib".to_string(), semver::Version::new(0, 9, 0));
    assert_eq!(filter.check("my-lib>=1.0.0", &ctx), FilterResult::NoMatch);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings library_version`
Expected: FAIL

**Step 3: Implement LibraryVersionFilter**

```rust
// Add to static_filters.rs after EnvironmentFilter
use semver::{Version, VersionReq};

/// library_version: "pkg>=1.0.0,pkg<2.0.0" against ctx.libraries_versions
pub struct LibraryVersionFilter;

impl StaticFilter for LibraryVersionFilter {
    fn name(&self) -> &'static str {
        "library_version"
    }

    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult {
        // Parse pattern like "pkg>=1.0.0,pkg<2.0.0" or "pkg=1.2.3"
        for spec in pattern.split(',') {
            let spec = spec.trim();
            if spec.is_empty() {
                continue;
            }

            // Find the operator position
            let (pkg_name, op, version_str) = if let Some(pos) = spec.find(">=") {
                (&spec[..pos], ">=", &spec[pos + 2..])
            } else if let Some(pos) = spec.find("<=") {
                (&spec[..pos], "<=", &spec[pos + 2..])
            } else if let Some(pos) = spec.find('>') {
                (&spec[..pos], ">", &spec[pos + 1..])
            } else if let Some(pos) = spec.find('<') {
                (&spec[..pos], "<", &spec[pos + 1..])
            } else if let Some(pos) = spec.find('=') {
                (&spec[..pos], "=", &spec[pos + 1..])
            } else {
                return FilterResult::NoMatch;
            };

            let pkg_name = pkg_name.trim();
            let version_str = version_str.trim();

            // Get installed version
            let installed = match ctx.libraries_versions.get(pkg_name) {
                Some(v) => v,
                None => return FilterResult::NoMatch,
            };

            // Parse required version
            let required = match Version::parse(version_str) {
                Ok(v) => v,
                Err(_) => return FilterResult::NoMatch,
            };

            // Check condition
            let matches = match op {
                ">=" => installed >= &required,
                "<=" => installed <= &required,
                ">" => installed > &required,
                "<" => installed < &required,
                "=" => installed == &required,
                _ => false,
            };

            if !matches {
                return FilterResult::NoMatch;
            }
        }
        FilterResult::Match
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings library_version`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement LibraryVersionFilter"
```

---

### Task 2.5: Implement regex-based dynamic filters (url_path, host, email, ip)

**Files:**
- Modify: `lib/runtime-settings/src/filters/dynamic_filters.rs`

**Step 1: Write tests**

```rust
// lib/runtime-settings/src/filters/dynamic_filters.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_ctx_with_request(path: &str, headers: HashMap<String, String>) -> Context {
        Context {
            application: "app".to_string(),
            server: "server".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
            request: Some(Request {
                method: "GET".to_string(),
                path: path.to_string(),
                headers,
            }),
            custom: HashMap::new(),
        }
    }

    #[test]
    fn test_url_path_filter_match() {
        let filter = UrlPathFilter;
        let ctx = make_ctx_with_request("/api/users", HashMap::new());
        assert_eq!(filter.check("/api/.*", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_url_path_filter_no_match() {
        let filter = UrlPathFilter;
        let ctx = make_ctx_with_request("/admin/users", HashMap::new());
        assert_eq!(filter.check("/api/.*", &ctx), FilterResult::NoMatch);
    }

    #[test]
    fn test_url_path_filter_no_request() {
        let filter = UrlPathFilter;
        let ctx = Context::default();
        assert_eq!(filter.check("/api/.*", &ctx), FilterResult::NotApplicable);
    }

    #[test]
    fn test_host_filter_match() {
        let filter = HostFilter;
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), "api.example.com".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert_eq!(filter.check("api\\.example\\.com", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_email_filter_match() {
        let filter = EmailFilter;
        let mut headers = HashMap::new();
        headers.insert("x-real-email".to_string(), "user@cian.ru".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert_eq!(filter.check(".*@cian\\.ru", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_ip_filter_match() {
        let filter = IpFilter;
        let mut headers = HashMap::new();
        headers.insert("x-real-ip".to_string(), "192.168.1.100".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert_eq!(filter.check("192\\.168\\..*", &ctx), FilterResult::Match);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings filters::dynamic_filters`
Expected: FAIL

**Step 3: Implement dynamic filters**

```rust
// lib/runtime-settings/src/filters/dynamic_filters.rs
use super::{DynamicFilter, FilterResult};
use crate::context::{Context, Request};
use regex::RegexBuilder;

/// Helper to check regex pattern against value (case-insensitive, anchored)
fn check_regex(pattern: &str, value: &str) -> FilterResult {
    let anchored = format!("^(?:{})$", pattern);
    match RegexBuilder::new(&anchored)
        .case_insensitive(true)
        .build()
    {
        Ok(re) => {
            if re.is_match(value) {
                FilterResult::Match
            } else {
                FilterResult::NoMatch
            }
        }
        Err(_) => FilterResult::NoMatch,
    }
}

/// url-path: regex against ctx.request.path
pub struct UrlPathFilter;

impl DynamicFilter for UrlPathFilter {
    fn name(&self) -> &'static str {
        "url-path"
    }

    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult {
        match &ctx.request {
            Some(req) => check_regex(pattern, &req.path),
            None => FilterResult::NotApplicable,
        }
    }
}

/// host: regex against ctx.request.host()
pub struct HostFilter;

impl DynamicFilter for HostFilter {
    fn name(&self) -> &'static str {
        "host"
    }

    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult {
        match &ctx.request {
            Some(req) => match req.host() {
                Some(host) => check_regex(pattern, host),
                None => FilterResult::NotApplicable,
            },
            None => FilterResult::NotApplicable,
        }
    }
}

/// email: regex against ctx.request.email()
pub struct EmailFilter;

impl DynamicFilter for EmailFilter {
    fn name(&self) -> &'static str {
        "email"
    }

    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult {
        match &ctx.request {
            Some(req) => match req.email() {
                Some(email) => check_regex(pattern, email),
                None => FilterResult::NotApplicable,
            },
            None => FilterResult::NotApplicable,
        }
    }
}

/// ip: regex against ctx.request.ip()
pub struct IpFilter;

impl DynamicFilter for IpFilter {
    fn name(&self) -> &'static str {
        "ip"
    }

    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult {
        match &ctx.request {
            Some(req) => match req.ip() {
                Some(ip) => check_regex(pattern, ip),
                None => FilterResult::NotApplicable,
            },
            None => FilterResult::NotApplicable,
        }
    }
}

// ... tests at the bottom
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings filters::dynamic_filters`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement UrlPathFilter, HostFilter, EmailFilter, IpFilter"
```

---

### Task 2.6: Implement map-based dynamic filters (header, context)

**Files:**
- Modify: `lib/runtime-settings/src/filters/dynamic_filters.rs`

**Step 1: Add tests**

```rust
// Add to dynamic_filters.rs tests module
#[test]
fn test_header_filter_match() {
    let filter = HeaderFilter;
    let mut headers = HashMap::new();
    headers.insert("X-Feature".to_string(), "enabled".to_string());
    let ctx = make_ctx_with_request("/", headers);
    assert_eq!(filter.check("X-Feature=enabled", &ctx), FilterResult::Match);
}

#[test]
fn test_header_filter_case_insensitive() {
    let filter = HeaderFilter;
    let mut headers = HashMap::new();
    headers.insert("x-feature".to_string(), "enabled".to_string());
    let ctx = make_ctx_with_request("/", headers);
    assert_eq!(filter.check("X-Feature=enabled", &ctx), FilterResult::Match);
}

#[test]
fn test_context_filter_match() {
    let filter = ContextFilter;
    let mut ctx = Context::default();
    ctx.custom.insert("user_id".to_string(), "123".to_string());
    ctx.custom.insert("role".to_string(), "admin".to_string());
    assert_eq!(filter.check("user_id=123,role=admin", &ctx), FilterResult::Match);
}

#[test]
fn test_context_filter_regex_value() {
    let filter = ContextFilter;
    let mut ctx = Context::default();
    ctx.custom.insert("user_id".to_string(), "12345".to_string());
    assert_eq!(filter.check("user_id=123.*", &ctx), FilterResult::Match);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings header_filter`
Expected: FAIL

**Step 3: Implement HeaderFilter and ContextFilter**

```rust
// Add to dynamic_filters.rs after IpFilter

/// Helper to parse "KEY1=value1,KEY2=value2" and check against a map
fn check_map_filter(pattern: &str, map: &std::collections::HashMap<String, String>) -> FilterResult {
    for pair in pattern.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() != 2 {
            return FilterResult::NoMatch;
        }
        let key = parts[0].trim();
        let value_pattern = parts[1].trim();

        match map.get(key) {
            Some(actual_value) => {
                if check_regex(value_pattern, actual_value) != FilterResult::Match {
                    return FilterResult::NoMatch;
                }
            }
            None => return FilterResult::NoMatch,
        }
    }
    FilterResult::Match
}

/// Helper for case-insensitive header lookup
fn check_header_filter(pattern: &str, headers: &std::collections::HashMap<String, String>) -> FilterResult {
    // Build lowercase map for case-insensitive matching
    let headers_lower: std::collections::HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect();

    for pair in pattern.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() != 2 {
            return FilterResult::NoMatch;
        }
        let key = parts[0].trim().to_lowercase();
        let value_pattern = parts[1].trim();

        match headers_lower.get(&key) {
            Some(actual_value) => {
                if check_regex(value_pattern, actual_value) != FilterResult::Match {
                    return FilterResult::NoMatch;
                }
            }
            None => return FilterResult::NoMatch,
        }
    }
    FilterResult::Match
}

/// header: "KEY1=val1,KEY2=val2" against ctx.request.headers
pub struct HeaderFilter;

impl DynamicFilter for HeaderFilter {
    fn name(&self) -> &'static str {
        "header"
    }

    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult {
        match &ctx.request {
            Some(req) => check_header_filter(pattern, &req.headers),
            None => FilterResult::NotApplicable,
        }
    }
}

/// context: "KEY1=val1,KEY2=val2" against ctx.custom
pub struct ContextFilter;

impl DynamicFilter for ContextFilter {
    fn name(&self) -> &'static str {
        "context"
    }

    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult {
        check_map_filter(pattern, &ctx.custom)
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings filters::dynamic_filters`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement HeaderFilter and ContextFilter"
```

---

### Task 2.7: Implement ProbabilityFilter

**Files:**
- Modify: `lib/runtime-settings/src/filters/dynamic_filters.rs`

**Step 1: Add tests**

```rust
// Add to dynamic_filters.rs tests module
#[test]
fn test_probability_filter_zero() {
    let filter = ProbabilityFilter;
    let ctx = Context::default();
    // 0% should always NoMatch
    assert_eq!(filter.check("0", &ctx), FilterResult::NoMatch);
}

#[test]
fn test_probability_filter_hundred() {
    let filter = ProbabilityFilter;
    let ctx = Context::default();
    // 100% should always Match
    assert_eq!(filter.check("100", &ctx), FilterResult::Match);
}

#[test]
fn test_probability_filter_invalid() {
    let filter = ProbabilityFilter;
    let ctx = Context::default();
    assert_eq!(filter.check("abc", &ctx), FilterResult::NoMatch);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings probability`
Expected: FAIL

**Step 3: Implement ProbabilityFilter**

```rust
// Add to dynamic_filters.rs after ContextFilter
use rand::Rng;

/// probability: "25" — 25% chance of Match
pub struct ProbabilityFilter;

impl DynamicFilter for ProbabilityFilter {
    fn name(&self) -> &'static str {
        "probability"
    }

    fn check(&self, pattern: &str, _ctx: &Context) -> FilterResult {
        let probability: f64 = match pattern.parse() {
            Ok(p) => p,
            Err(_) => return FilterResult::NoMatch,
        };

        if probability <= 0.0 {
            return FilterResult::NoMatch;
        }
        if probability >= 100.0 {
            return FilterResult::Match;
        }

        let mut rng = rand::thread_rng();
        let roll: f64 = rng.gen_range(0.0..100.0);

        if roll < probability {
            FilterResult::Match
        } else {
            FilterResult::NoMatch
        }
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings probability`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement ProbabilityFilter"
```

---

### Task 2.8: Create filter registry and checker

**Files:**
- Modify: `lib/runtime-settings/src/filters/mod.rs`

**Step 1: Add tests**

```rust
// Add to lib/runtime-settings/src/filters/mod.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_check_static_filters_all_match() {
        let filters: HashMap<String, String> = [
            ("application".to_string(), "my-app".to_string()),
            ("server".to_string(), "server-1".to_string()),
        ].into();

        let ctx = StaticContext {
            application: "my-app".to_string(),
            server: "server-1".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };

        assert!(check_static_filters(&filters, &ctx));
    }

    #[test]
    fn test_check_static_filters_one_no_match() {
        let filters: HashMap<String, String> = [
            ("application".to_string(), "my-app".to_string()),
            ("server".to_string(), "other-server".to_string()),
        ].into();

        let ctx = StaticContext {
            application: "my-app".to_string(),
            server: "server-1".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };

        assert!(!check_static_filters(&filters, &ctx));
    }

    #[test]
    fn test_check_dynamic_filters_all_match() {
        let filters: HashMap<String, String> = [
            ("url-path".to_string(), "/api/.*".to_string()),
        ].into();

        let mut ctx = Context::default();
        ctx.request = Some(crate::context::Request {
            method: "GET".to_string(),
            path: "/api/users".to_string(),
            headers: HashMap::new(),
        });

        assert!(check_dynamic_filters(&filters, &ctx));
    }

    #[test]
    fn test_check_filters_ignores_unknown() {
        let filters: HashMap<String, String> = [
            ("unknown_filter".to_string(), "value".to_string()),
        ].into();

        let ctx = StaticContext {
            application: "app".to_string(),
            server: "server".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };

        // Unknown filters should be ignored (return true)
        assert!(check_static_filters(&filters, &ctx));
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings filters::tests`
Expected: FAIL

**Step 3: Implement filter registry**

```rust
// Add to lib/runtime-settings/src/filters/mod.rs after trait definitions
use std::collections::HashMap;
use lazy_static::lazy_static;

lazy_static! {
    static ref STATIC_FILTERS: Vec<Box<dyn StaticFilter>> = vec![
        Box::new(static_filters::ApplicationFilter),
        Box::new(static_filters::ServerFilter),
        Box::new(static_filters::EnvironmentFilter),
        Box::new(static_filters::McsRunEnvFilter),
        Box::new(static_filters::LibraryVersionFilter),
    ];

    static ref DYNAMIC_FILTERS: Vec<Box<dyn DynamicFilter>> = vec![
        Box::new(dynamic_filters::UrlPathFilter),
        Box::new(dynamic_filters::HostFilter),
        Box::new(dynamic_filters::EmailFilter),
        Box::new(dynamic_filters::IpFilter),
        Box::new(dynamic_filters::HeaderFilter),
        Box::new(dynamic_filters::ContextFilter),
        Box::new(dynamic_filters::ProbabilityFilter),
    ];

    static ref STATIC_FILTER_NAMES: Vec<&'static str> = STATIC_FILTERS.iter().map(|f| f.name()).collect();
    static ref DYNAMIC_FILTER_NAMES: Vec<&'static str> = DYNAMIC_FILTERS.iter().map(|f| f.name()).collect();
}

/// Check all static filters. Returns true if all match (or NotApplicable).
pub fn check_static_filters(filters: &HashMap<String, String>, ctx: &StaticContext) -> bool {
    for (name, pattern) in filters {
        // Skip dynamic filters
        if DYNAMIC_FILTER_NAMES.contains(&name.as_str()) {
            continue;
        }

        // Find matching static filter
        if let Some(filter) = STATIC_FILTERS.iter().find(|f| f.name() == name) {
            match filter.check(pattern, ctx) {
                FilterResult::Match | FilterResult::NotApplicable => continue,
                FilterResult::NoMatch => return false,
            }
        }
        // Unknown filter - ignore
    }
    true
}

/// Check all dynamic filters. Returns true if all match (or NotApplicable).
pub fn check_dynamic_filters(filters: &HashMap<String, String>, ctx: &Context) -> bool {
    for (name, pattern) in filters {
        // Skip static filters
        if STATIC_FILTER_NAMES.contains(&name.as_str()) {
            continue;
        }

        // Find matching dynamic filter
        if let Some(filter) = DYNAMIC_FILTERS.iter().find(|f| f.name() == name) {
            match filter.check(pattern, ctx) {
                FilterResult::Match | FilterResult::NotApplicable => continue,
                FilterResult::NoMatch => return false,
            }
        }
        // Unknown filter - ignore
    }
    true
}

// ... tests at the bottom
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings filters`
Expected: PASS

**Step 5: Update lib.rs exports**

```rust
// lib/runtime-settings/src/lib.rs
pub use filters::{check_dynamic_filters, check_static_filters, FilterResult};
```

**Step 6: Commit**

```bash
git add -A && git commit -m "Add filter registry and check functions"
```

---

## Phase 3: Providers

### Task 3.1: Create providers module structure

**Files:**
- Create: `lib/runtime-settings/src/providers/mod.rs`
- Create: `lib/runtime-settings/src/providers/env.rs`
- Create: `lib/runtime-settings/src/providers/file.rs`
- Create: `lib/runtime-settings/src/providers/mcs.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Create provider trait**

```rust
// lib/runtime-settings/src/providers/mod.rs
pub mod env;
pub mod file;
pub mod mcs;

use crate::entities::{Setting, SettingKey};
use crate::error::SettingsError;
use async_trait::async_trait;

/// Response from a settings provider
#[derive(Debug, Clone, Default)]
pub struct ProviderResponse {
    pub settings: Vec<Setting>,
    pub deleted: Vec<SettingKey>,
    pub version: String,
}

/// Trait for settings providers
#[async_trait]
pub trait SettingsProvider: Send + Sync {
    /// Load settings. Returns settings, deleted keys, and new version.
    async fn load(&self, current_version: &str) -> Result<ProviderResponse, SettingsError>;

    /// Default priority for settings from this provider
    fn default_priority(&self) -> i64;

    /// Provider name for logging
    fn name(&self) -> &'static str;
}

pub use env::EnvProvider;
pub use file::FileProvider;
pub use mcs::McsProvider;
```

**Step 2: Create skeleton files**

```rust
// lib/runtime-settings/src/providers/env.rs
use super::{ProviderResponse, SettingsProvider};
use crate::error::SettingsError;
use async_trait::async_trait;

pub struct EnvProvider;

// lib/runtime-settings/src/providers/file.rs
use super::{ProviderResponse, SettingsProvider};
use crate::error::SettingsError;
use async_trait::async_trait;
use std::path::PathBuf;

pub struct FileProvider {
    path: PathBuf,
}

// lib/runtime-settings/src/providers/mcs.rs
use super::{ProviderResponse, SettingsProvider};
use crate::error::SettingsError;
use async_trait::async_trait;

pub struct McsProvider;
```

**Step 3: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs
pub mod context;
pub mod entities;
pub mod error;
pub mod filters;
pub mod providers;

pub use context::{Context, Request, StaticContext};
pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
pub use filters::{check_dynamic_filters, check_static_filters, FilterResult};
pub use providers::{EnvProvider, FileProvider, McsProvider, ProviderResponse, SettingsProvider};
```

**Step 4: Verify it compiles**

Run: `cargo check -p runtime-settings`
Expected: Success

**Step 5: Commit**

```bash
git add -A && git commit -m "Add providers module structure"
```

---

### Task 3.2: Implement EnvProvider

**Files:**
- Modify: `lib/runtime-settings/src/providers/env.rs`

**Step 1: Write tests**

```rust
// lib/runtime-settings/src/providers/env.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_env_provider_loads_env_vars() {
        let mut env = std::collections::HashMap::new();
        env.insert("MY_VAR".to_string(), "my_value".to_string());
        env.insert("MY_NUM".to_string(), "123".to_string());

        let provider = EnvProvider::new(env);
        let response = provider.load("").await.unwrap();

        assert!(response.settings.iter().any(|s| s.key == "MY_VAR"));
        assert!(response.settings.iter().any(|s| s.key == "MY_NUM"));
    }

    #[tokio::test]
    async fn test_env_provider_parses_json() {
        let mut env = std::collections::HashMap::new();
        env.insert("JSON_VAR".to_string(), r#"{"key": "value"}"#.to_string());

        let provider = EnvProvider::new(env);
        let response = provider.load("").await.unwrap();

        let setting = response.settings.iter().find(|s| s.key == "JSON_VAR").unwrap();
        assert_eq!(setting.value, serde_json::json!({"key": "value"}));
    }

    #[tokio::test]
    async fn test_env_provider_string_fallback() {
        let mut env = std::collections::HashMap::new();
        env.insert("STR_VAR".to_string(), "not json".to_string());

        let provider = EnvProvider::new(env);
        let response = provider.load("").await.unwrap();

        let setting = response.settings.iter().find(|s| s.key == "STR_VAR").unwrap();
        assert_eq!(setting.value, serde_json::json!("not json"));
    }

    #[tokio::test]
    async fn test_env_provider_priority() {
        let provider = EnvProvider::new(std::collections::HashMap::new());
        assert_eq!(provider.default_priority(), -1_000_000_000_000_000_000);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings providers::env`
Expected: FAIL

**Step 3: Implement EnvProvider**

```rust
// lib/runtime-settings/src/providers/env.rs
use super::{ProviderResponse, SettingsProvider};
use crate::entities::Setting;
use crate::error::SettingsError;
use async_trait::async_trait;
use std::collections::HashMap;

const ENV_PRIORITY: i64 = -1_000_000_000_000_000_000;

pub struct EnvProvider {
    environ: HashMap<String, String>,
}

impl EnvProvider {
    /// Create with custom environment (for testing)
    pub fn new(environ: HashMap<String, String>) -> Self {
        Self { environ }
    }

    /// Create with actual OS environment
    pub fn from_env() -> Self {
        Self {
            environ: std::env::vars().collect(),
        }
    }
}

#[async_trait]
impl SettingsProvider for EnvProvider {
    async fn load(&self, _current_version: &str) -> Result<ProviderResponse, SettingsError> {
        let settings: Vec<Setting> = self
            .environ
            .iter()
            .map(|(key, value)| {
                // Try to parse as JSON, fall back to string
                let json_value = serde_json::from_str(value)
                    .unwrap_or_else(|_| serde_json::Value::String(value.clone()));

                Setting {
                    key: key.clone(),
                    priority: ENV_PRIORITY,
                    filter: HashMap::new(),
                    value: json_value,
                }
            })
            .collect();

        Ok(ProviderResponse {
            settings,
            deleted: vec![],
            version: String::new(),
        })
    }

    fn default_priority(&self) -> i64 {
        ENV_PRIORITY
    }

    fn name(&self) -> &'static str {
        "env"
    }
}

// ... tests at the bottom
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings providers::env`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement EnvProvider"
```

---

### Task 3.3: Implement FileProvider

**Files:**
- Modify: `lib/runtime-settings/src/providers/file.rs`

**Step 1: Write tests**

```rust
// lib/runtime-settings/src/providers/file.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_file_provider_loads_json() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"[{{"key": "TEST_KEY", "priority": 100, "value": "test"}}]"#).unwrap();

        let provider = FileProvider::new(file.path().to_path_buf());
        let response = provider.load("").await.unwrap();

        assert_eq!(response.settings.len(), 1);
        assert_eq!(response.settings[0].key, "TEST_KEY");
        assert_eq!(response.settings[0].priority, 100);
    }

    #[tokio::test]
    async fn test_file_provider_default_priority() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"[{{"key": "KEY", "value": 123}}]"#).unwrap();

        let provider = FileProvider::new(file.path().to_path_buf());
        let response = provider.load("").await.unwrap();

        assert_eq!(response.settings[0].priority, 1_000_000_000_000_000_000);
    }

    #[tokio::test]
    async fn test_file_provider_missing_file() {
        let provider = FileProvider::new("/nonexistent/path.json".into());
        let result = provider.load("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_provider_json5_comments() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"[
            // This is a comment
            {{"key": "KEY", "value": "val"}}
        ]"#).unwrap();

        let provider = FileProvider::new(file.path().to_path_buf());
        let response = provider.load("").await.unwrap();

        assert_eq!(response.settings.len(), 1);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings providers::file`
Expected: FAIL

**Step 3: Implement FileProvider**

```rust
// lib/runtime-settings/src/providers/file.rs
use super::{ProviderResponse, SettingsProvider};
use crate::entities::Setting;
use crate::error::SettingsError;
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

const FILE_DEFAULT_PRIORITY: i64 = 1_000_000_000_000_000_000;

/// Setting as stored in file (priority is optional)
#[derive(Debug, Deserialize)]
struct FileSetting {
    key: String,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    filter: HashMap<String, String>,
    value: serde_json::Value,
}

pub struct FileProvider {
    path: PathBuf,
}

impl FileProvider {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create from RUNTIME_SETTINGS_FILE_PATH env var or default
    pub fn from_env() -> Self {
        let path = std::env::var("RUNTIME_SETTINGS_FILE_PATH")
            .unwrap_or_else(|_| "runtime-settings.json".to_string());
        Self::new(PathBuf::from(path))
    }
}

#[async_trait]
impl SettingsProvider for FileProvider {
    async fn load(&self, _current_version: &str) -> Result<ProviderResponse, SettingsError> {
        let content = tokio::fs::read_to_string(&self.path).await?;

        // Parse as JSON5 to support comments
        let file_settings: Vec<FileSetting> = json5::from_str(&content)
            .map_err(|e| SettingsError::JsonParse(serde_json::Error::io(
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
            )))?;

        let settings = file_settings
            .into_iter()
            .map(|fs| Setting {
                key: fs.key,
                priority: fs.priority.unwrap_or(FILE_DEFAULT_PRIORITY),
                filter: fs.filter,
                value: fs.value,
            })
            .collect();

        Ok(ProviderResponse {
            settings,
            deleted: vec![],
            version: String::new(),
        })
    }

    fn default_priority(&self) -> i64 {
        FILE_DEFAULT_PRIORITY
    }

    fn name(&self) -> &'static str {
        "file"
    }
}

// ... tests at the bottom
```

**Step 4: Add tempfile to dev-dependencies**

```toml
# Add to lib/runtime-settings/Cargo.toml [dev-dependencies]
tempfile = "3"
```

**Step 5: Run tests**

Run: `cargo test -p runtime-settings providers::file`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "Implement FileProvider"
```

---

### Task 3.4: Implement McsProvider

**Files:**
- Modify: `lib/runtime-settings/src/providers/mcs.rs`

**Step 1: Write tests**

```rust
// lib/runtime-settings/src/providers/mcs.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcs_request_serialization() {
        let req = McsRequest {
            runtime: "rust".to_string(),
            version: "42".to_string(),
            application: Some("my-app".to_string()),
            mcs_run_env: Some("PROD".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""runtime":"rust""#));
        assert!(json.contains(r#""version":"42""#));
    }

    #[test]
    fn test_mcs_provider_default_url() {
        let provider = McsProvider::new(
            "http://test.local".to_string(),
            "app".to_string(),
            None,
        );
        assert_eq!(provider.name(), "mcs");
    }
}
```

**Step 2: Implement McsProvider**

```rust
// lib/runtime-settings/src/providers/mcs.rs
use super::{ProviderResponse, SettingsProvider};
use crate::entities::McsResponse;
use crate::error::SettingsError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const DEFAULT_MCS_BASE_URL: &str = "http://master.runtime-settings.dev3.cian.ru";

#[derive(Debug, Serialize)]
struct McsRequest {
    runtime: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    application: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mcs_run_env: Option<String>,
}

pub struct McsProvider {
    base_url: String,
    application: String,
    mcs_run_env: Option<String>,
    client: reqwest::Client,
}

impl McsProvider {
    pub fn new(base_url: String, application: String, mcs_run_env: Option<String>) -> Self {
        Self {
            base_url,
            application,
            mcs_run_env,
            client: reqwest::Client::new(),
        }
    }

    /// Create from environment variables
    pub fn from_env(application: String) -> Self {
        let base_url = std::env::var("RUNTIME_SETTINGS_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_MCS_BASE_URL.to_string());
        let mcs_run_env = std::env::var("MCS_RUN_ENV").ok();
        Self::new(base_url, application, mcs_run_env)
    }
}

#[async_trait]
impl SettingsProvider for McsProvider {
    async fn load(&self, current_version: &str) -> Result<ProviderResponse, SettingsError> {
        let url = format!("{}/v3/get-runtime-settings/", self.base_url);

        let request = McsRequest {
            runtime: "rust".to_string(),
            version: current_version.to_string(),
            application: Some(self.application.clone()),
            mcs_run_env: self.mcs_run_env.clone(),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SettingsError::McsResponse {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let mcs_response: McsResponse = response.json().await?;

        Ok(ProviderResponse {
            settings: mcs_response.settings,
            deleted: mcs_response.deleted,
            version: mcs_response.version,
        })
    }

    fn default_priority(&self) -> i64 {
        0 // MCS settings have their own priority
    }

    fn name(&self) -> &'static str {
        "mcs"
    }
}

// ... tests at the bottom
```

**Step 3: Run tests**

Run: `cargo test -p runtime-settings providers::mcs`
Expected: PASS

**Step 4: Commit**

```bash
git add -A && git commit -m "Implement McsProvider"
```

---

## Phase 4: Scoped Contexts

### Task 4.1: Implement scoped context module

**Files:**
- Create: `lib/runtime-settings/src/scoped.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Write tests**

```rust
// lib/runtime-settings/src/scoped.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_local_context() {
        let ctx = Context {
            application: "test-app".to_string(),
            ..Default::default()
        };

        {
            let _guard = set_thread_context(ctx.clone());
            let current = current_context().unwrap();
            assert_eq!(current.application, "test-app");
        }

        // After guard dropped, context should be None
        assert!(current_context().is_none());
    }

    #[test]
    fn test_thread_local_request() {
        let req = Request {
            method: "POST".to_string(),
            path: "/api".to_string(),
            headers: std::collections::HashMap::new(),
        };

        {
            let _guard = set_thread_request(req.clone());
            let current = current_request().unwrap();
            assert_eq!(current.method, "POST");
        }

        assert!(current_request().is_none());
    }

    #[test]
    fn test_nested_context_guards() {
        let ctx1 = Context {
            application: "app1".to_string(),
            ..Default::default()
        };
        let ctx2 = Context {
            application: "app2".to_string(),
            ..Default::default()
        };

        {
            let _guard1 = set_thread_context(ctx1);
            assert_eq!(current_context().unwrap().application, "app1");

            {
                let _guard2 = set_thread_context(ctx2);
                assert_eq!(current_context().unwrap().application, "app2");
            }

            // After inner guard dropped, should restore outer context
            assert_eq!(current_context().unwrap().application, "app1");
        }

        assert!(current_context().is_none());
    }

    #[tokio::test]
    async fn test_task_local_context() {
        let ctx = Context {
            application: "async-app".to_string(),
            ..Default::default()
        };

        let result = with_task_context(ctx, async {
            current_context().unwrap().application.clone()
        }).await;

        assert_eq!(result, "async-app");
    }

    #[tokio::test]
    async fn test_task_local_priority_over_thread_local() {
        let thread_ctx = Context {
            application: "thread-app".to_string(),
            ..Default::default()
        };
        let task_ctx = Context {
            application: "task-app".to_string(),
            ..Default::default()
        };

        let _guard = set_thread_context(thread_ctx);

        let result = with_task_context(task_ctx, async {
            current_context().unwrap().application.clone()
        }).await;

        // Task-local should win
        assert_eq!(result, "task-app");

        // Outside task-local scope, thread-local should be visible
        assert_eq!(current_context().unwrap().application, "thread-app");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p runtime-settings scoped`
Expected: FAIL

**Step 3: Implement scoped module**

```rust
// lib/runtime-settings/src/scoped.rs
use crate::context::{Context, Request};
use std::cell::RefCell;

tokio::task_local! {
    static TASK_CONTEXT: Option<Context>;
    static TASK_REQUEST: Option<Request>;
}

thread_local! {
    static THREAD_CONTEXT: RefCell<Option<Context>> = RefCell::new(None);
    static THREAD_REQUEST: RefCell<Option<Request>> = RefCell::new(None);
}

/// Get current context (task-local takes priority over thread-local)
pub fn current_context() -> Option<Context> {
    TASK_CONTEXT
        .try_with(|c| c.clone())
        .ok()
        .flatten()
        .or_else(|| THREAD_CONTEXT.with(|c| c.borrow().clone()))
}

/// Get current request (task-local takes priority over thread-local)
pub fn current_request() -> Option<Request> {
    TASK_REQUEST
        .try_with(|r| r.clone())
        .ok()
        .flatten()
        .or_else(|| THREAD_REQUEST.with(|r| r.borrow().clone()))
}

/// Guard that restores previous context on drop
pub struct ContextGuard {
    previous: Option<Context>,
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        THREAD_CONTEXT.with(|c| {
            *c.borrow_mut() = self.previous.take();
        });
    }
}

/// Guard that restores previous request on drop
pub struct RequestGuard {
    previous: Option<Request>,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        THREAD_REQUEST.with(|r| {
            *r.borrow_mut() = self.previous.take();
        });
    }
}

/// Set thread-local context, returns guard that restores previous on drop
pub fn set_thread_context(ctx: Context) -> ContextGuard {
    let previous = THREAD_CONTEXT.with(|c| c.borrow_mut().replace(ctx));
    ContextGuard { previous }
}

/// Set thread-local request, returns guard that restores previous on drop
pub fn set_thread_request(req: Request) -> RequestGuard {
    let previous = THREAD_REQUEST.with(|r| r.borrow_mut().replace(req));
    RequestGuard { previous }
}

/// Execute async closure with task-local context
pub async fn with_task_context<F, T>(ctx: Context, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TASK_CONTEXT.scope(Some(ctx), f).await
}

/// Execute async closure with task-local request
pub async fn with_task_request<F, T>(req: Request, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TASK_REQUEST.scope(Some(req), f).await
}

// ... tests at the bottom
```

**Step 4: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs
pub mod scoped;

pub use scoped::{
    current_context, current_request, set_thread_context, set_thread_request,
    with_task_context, with_task_request, ContextGuard, RequestGuard,
};
```

**Step 5: Run tests**

Run: `cargo test -p runtime-settings scoped`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "Implement scoped context module"
```

---

## Phase 5: Secrets (Vault Integration)

### Task 5.1: Create secrets module structure

**Files:**
- Create: `lib/runtime-settings/src/secrets/mod.rs`
- Create: `lib/runtime-settings/src/secrets/resolver.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Create secrets module**

```rust
// lib/runtime-settings/src/secrets/mod.rs
pub mod resolver;

use crate::error::SettingsError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};

pub use resolver::resolve_secrets;

/// Cached secret with metadata
struct CachedSecret {
    value: serde_json::Value,
    lease_id: Option<String>,
    lease_duration: Option<Duration>,
    renewable: bool,
    fetched_at: Instant,
}

pub struct SecretsService {
    client: Option<VaultClient>,
    cache: RwLock<HashMap<String, CachedSecret>>,
    refresh_intervals: HashMap<String, Duration>,
}

impl SecretsService {
    /// Create without Vault (secrets will fail)
    pub fn new_without_vault() -> Self {
        Self {
            client: None,
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::default_refresh_intervals(),
        }
    }

    /// Create with Vault client
    pub fn new(client: VaultClient) -> Self {
        Self {
            client: Some(client),
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::default_refresh_intervals(),
        }
    }

    /// Create Vault client from environment
    pub fn from_env() -> Result<Self, SettingsError> {
        let address = std::env::var("VAULT_ADDR")
            .unwrap_or_else(|_| "http://127.0.0.1:8200".to_string());
        let token = std::env::var("VAULT_TOKEN").ok();

        if token.is_none() {
            return Ok(Self::new_without_vault());
        }

        let settings = VaultClientSettingsBuilder::default()
            .address(&address)
            .token(token.unwrap())
            .build()
            .map_err(|e| SettingsError::Vault(e.to_string()))?;

        let client = VaultClient::new(settings)
            .map_err(|e| SettingsError::Vault(e.to_string()))?;

        Ok(Self::new(client))
    }

    fn default_refresh_intervals() -> HashMap<String, Duration> {
        let mut intervals = HashMap::new();
        intervals.insert("kafka-certificates".to_string(), Duration::from_secs(600));
        intervals.insert("interservice-auth".to_string(), Duration::from_secs(60));
        intervals
    }

    /// Get secret value by path and key
    pub async fn get(&self, path: &str, key: &str) -> Result<serde_json::Value, SettingsError> {
        let client = self.client.as_ref()
            .ok_or(SettingsError::SecretWithoutVault)?;

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(path) {
                if let Some(value) = cached.value.get(key) {
                    return Ok(value.clone());
                }
            }
        }

        // Fetch from Vault
        let secret: serde_json::Value = vaultrs::kv2::read(client, "secret", path)
            .await
            .map_err(|e| SettingsError::Vault(e.to_string()))?;

        // Cache it
        {
            let mut cache = self.cache.write().await;
            cache.insert(path.to_string(), CachedSecret {
                value: secret.clone(),
                lease_id: None,
                lease_duration: None,
                renewable: false,
                fetched_at: Instant::now(),
            });
        }

        secret.get(key)
            .cloned()
            .ok_or_else(|| SettingsError::SecretKeyNotFound {
                path: path.to_string(),
                key: key.to_string(),
            })
    }

    /// Refresh all cached secrets
    pub async fn refresh(&self) -> Result<(), SettingsError> {
        // TODO: Implement lease renewal and refresh logic
        Ok(())
    }
}
```

**Step 2: Create resolver**

```rust
// lib/runtime-settings/src/secrets/resolver.rs
use crate::error::SettingsError;
use super::SecretsService;

/// Recursively resolve {"$secret": "path:key"} references in a JSON value
pub async fn resolve_secrets(
    value: &serde_json::Value,
    secrets: &SecretsService,
) -> Result<serde_json::Value, SettingsError> {
    match value {
        serde_json::Value::Object(map) => {
            // Check if this is a secret reference
            if map.len() == 1 {
                if let Some(serde_json::Value::String(reference)) = map.get("$secret") {
                    return resolve_secret_reference(reference, secrets).await;
                }
            }

            // Recursively resolve nested objects
            let mut result = serde_json::Map::new();
            for (k, v) in map {
                result.insert(k.clone(), resolve_secrets(v, secrets).await?);
            }
            Ok(serde_json::Value::Object(result))
        }
        serde_json::Value::Array(arr) => {
            let mut result = Vec::new();
            for item in arr {
                result.push(resolve_secrets(item, secrets).await?);
            }
            Ok(serde_json::Value::Array(result))
        }
        _ => Ok(value.clone()),
    }
}

/// Resolve a single secret reference like "path/to/secret:key"
async fn resolve_secret_reference(
    reference: &str,
    secrets: &SecretsService,
) -> Result<serde_json::Value, SettingsError> {
    let parts: Vec<&str> = reference.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(SettingsError::InvalidSecretReference {
            reference: reference.to_string(),
        });
    }

    let path = parts[0];
    let key = parts[1];

    secrets.get(path, key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_no_secrets() {
        let secrets = SecretsService::new_without_vault();
        let value = serde_json::json!({"host": "localhost", "port": 5432});

        let resolved = resolve_secrets(&value, &secrets).await.unwrap();
        assert_eq!(resolved, value);
    }

    #[tokio::test]
    async fn test_resolve_secret_without_vault() {
        let secrets = SecretsService::new_without_vault();
        let value = serde_json::json!({"password": {"$secret": "db/creds:password"}});

        let result = resolve_secrets(&value, &secrets).await;
        assert!(matches!(result, Err(SettingsError::SecretWithoutVault)));
    }

    #[tokio::test]
    async fn test_invalid_secret_reference() {
        let secrets = SecretsService::new_without_vault();
        let value = serde_json::json!({"password": {"$secret": "invalid-no-colon"}});

        let result = resolve_secrets(&value, &secrets).await;
        assert!(matches!(result, Err(SettingsError::InvalidSecretReference { .. })));
    }
}
```

**Step 3: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs
pub mod secrets;

pub use secrets::{resolve_secrets, SecretsService};
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings secrets`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Add secrets module with Vault integration"
```

---

## Phase 6: Watchers

### Task 6.1: Implement watchers module

**Files:**
- Create: `lib/runtime-settings/src/watchers.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Write tests**

```rust
// lib/runtime-settings/src/watchers.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_add_and_remove_watcher() {
        let service = WatchersService::new();

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let id = service.add("MY_KEY", Box::new(move |_, _| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        // Verify watcher was added
        let watchers = service.watchers.read().unwrap();
        assert!(watchers.contains_key("MY_KEY"));
        drop(watchers);

        // Remove and verify
        service.remove(id);
        let watchers = service.watchers.read().unwrap();
        assert!(watchers.get("MY_KEY").map_or(true, |v| v.is_empty()));
    }

    #[tokio::test]
    async fn test_check_triggers_on_change() {
        let service = WatchersService::new();

        let called = Arc::new(AtomicU32::new(0));
        let called_clone = called.clone();

        service.add("KEY", Box::new(move |old, new| {
            assert!(old.is_none());
            assert_eq!(new, Some(serde_json::json!("new_value")));
            called_clone.fetch_add(1, Ordering::SeqCst);
        }));

        // Simulate setting initial value
        {
            let mut snapshot = service.snapshot.write().unwrap();
            // No initial value
        }

        // Check with new value
        let mut current_values = HashMap::new();
        current_values.insert("KEY".to_string(), serde_json::json!("new_value"));

        service.check(&current_values).await;

        assert_eq!(called.load(Ordering::SeqCst), 1);
    }
}
```

**Step 2: Implement watchers**

```rust
// lib/runtime-settings/src/watchers.rs
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Unique identifier for a watcher
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatcherId(u64);

static NEXT_WATCHER_ID: AtomicU64 = AtomicU64::new(0);

impl WatcherId {
    fn next() -> Self {
        Self(NEXT_WATCHER_ID.fetch_add(1, Ordering::SeqCst))
    }
}

/// Sync watcher callback
pub type Watcher = Box<dyn Fn(Option<serde_json::Value>, Option<serde_json::Value>) + Send + Sync>;

struct WatcherEntry {
    id: WatcherId,
    callback: Watcher,
}

pub struct WatchersService {
    watchers: RwLock<HashMap<String, Vec<WatcherEntry>>>,
    snapshot: RwLock<HashMap<String, serde_json::Value>>,
}

impl WatchersService {
    pub fn new() -> Self {
        Self {
            watchers: RwLock::new(HashMap::new()),
            snapshot: RwLock::new(HashMap::new()),
        }
    }

    /// Add a watcher for a key
    pub fn add(&self, key: &str, callback: Watcher) -> WatcherId {
        let id = WatcherId::next();
        let entry = WatcherEntry { id, callback };

        let mut watchers = self.watchers.write().unwrap();
        watchers
            .entry(key.to_string())
            .or_insert_with(Vec::new)
            .push(entry);

        id
    }

    /// Remove a watcher by ID
    pub fn remove(&self, id: WatcherId) {
        let mut watchers = self.watchers.write().unwrap();
        for entries in watchers.values_mut() {
            entries.retain(|e| e.id != id);
        }
    }

    /// Check for changes and notify watchers
    pub async fn check(&self, current_values: &HashMap<String, serde_json::Value>) {
        let watchers = self.watchers.read().unwrap();
        let mut snapshot = self.snapshot.write().unwrap();

        for (key, entries) in watchers.iter() {
            let old_value = snapshot.get(key).cloned();
            let new_value = current_values.get(key).cloned();

            if old_value != new_value {
                // Update snapshot
                if let Some(ref v) = new_value {
                    snapshot.insert(key.clone(), v.clone());
                } else {
                    snapshot.remove(key);
                }

                // Notify watchers
                for entry in entries {
                    // Catch panics in watchers
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (entry.callback)(old_value.clone(), new_value.clone());
                    }));
                    if let Err(e) = result {
                        tracing::error!("Watcher for key '{}' panicked: {:?}", key, e);
                    }
                }
            }
        }
    }

    /// Update snapshot without notifying (for initialization)
    pub fn update_snapshot(&self, key: &str, value: serde_json::Value) {
        let mut snapshot = self.snapshot.write().unwrap();
        snapshot.insert(key.to_string(), value);
    }
}

impl Default for WatchersService {
    fn default() -> Self {
        Self::new()
    }
}

// ... tests at the bottom
```

**Step 3: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs
pub mod watchers;

pub use watchers::{Watcher, WatcherId, WatchersService};
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings watchers`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement watchers module"
```

---

## Phase 7: RuntimeSettings Main Structure

### Task 7.1: Create RuntimeSettings with builder

**Files:**
- Create: `lib/runtime-settings/src/settings.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Write tests**

```rust
// lib/runtime-settings/src/settings.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let settings = RuntimeSettingsBuilder::new()
            .application("test-app")
            .server("test-server")
            .build()
            .unwrap();

        assert_eq!(settings.static_context.application, "test-app");
        assert_eq!(settings.static_context.server, "test-server");
    }

    #[test]
    fn test_builder_with_library_version() {
        let settings = RuntimeSettingsBuilder::new()
            .application("app")
            .library_version("my-lib", "1.2.3")
            .build()
            .unwrap();

        let version = settings.static_context.libraries_versions.get("my-lib").unwrap();
        assert_eq!(version.to_string(), "1.2.3");
    }

    #[tokio::test]
    async fn test_get_requires_context() {
        let settings = RuntimeSettingsBuilder::new()
            .application("app")
            .build()
            .unwrap();

        // This should panic because no context is set
        let result = std::panic::catch_unwind(|| {
            let _: Option<String> = settings.get("KEY");
        });
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_with_context() {
        let settings = RuntimeSettingsBuilder::new()
            .application("app")
            .mcs_enabled(false)  // Don't try to connect to MCS
            .build()
            .unwrap();

        let ctx = Context {
            application: "app".to_string(),
            ..Default::default()
        };
        let _guard = set_thread_context(ctx);

        // Should not panic, just return None (no settings loaded)
        let result: Option<String> = settings.get("NONEXISTENT");
        assert!(result.is_none());
    }
}
```

**Step 2: Implement RuntimeSettings**

```rust
// lib/runtime-settings/src/settings.rs
use crate::context::{Context, Request, StaticContext};
use crate::entities::{Setting, SettingKey};
use crate::error::SettingsError;
use crate::filters::{check_dynamic_filters, check_static_filters};
use crate::providers::{EnvProvider, FileProvider, McsProvider, ProviderResponse, SettingsProvider};
use crate::scoped::{current_context, current_request, set_thread_context, set_thread_request, ContextGuard, RequestGuard, with_task_context, with_task_request};
use crate::secrets::{resolve_secrets, SecretsService};
use crate::watchers::{Watcher, WatcherId, WatchersService};

use semver::Version;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Internal state
struct SettingsState {
    version: String,
    /// key -> Vec<Setting> sorted by priority descending
    settings: HashMap<String, Vec<Setting>>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            version: "0".to_string(),
            settings: HashMap::new(),
        }
    }
}

pub struct RuntimeSettings {
    providers: Vec<Box<dyn SettingsProvider>>,
    state: RwLock<SettingsState>,
    secrets: SecretsService,
    watchers: WatchersService,
    pub(crate) static_context: StaticContext,
}

impl RuntimeSettings {
    /// Create a new builder
    pub fn builder() -> RuntimeSettingsBuilder {
        RuntimeSettingsBuilder::new()
    }

    /// Initialize settings from all providers
    pub async fn init(&self) -> Result<(), SettingsError> {
        for provider in &self.providers {
            match provider.load("0").await {
                Ok(response) => {
                    self.merge_settings(response).await;
                    tracing::info!("Loaded {} settings from {}",
                        self.state.read().await.settings.len(),
                        provider.name());
                }
                Err(e) => {
                    tracing::warn!("Failed to load settings from {}: {}", provider.name(), e);
                }
            }
        }
        Ok(())
    }

    /// Refresh settings from MCS provider
    pub async fn refresh(&self) -> Result<(), SettingsError> {
        let current_version = self.state.read().await.version.clone();

        // Only refresh from MCS
        for provider in &self.providers {
            if provider.name() == "mcs" {
                let response = provider.load(&current_version).await?;
                self.merge_settings(response).await;
            }
        }

        // Refresh secrets
        self.secrets.refresh().await?;

        // Check watchers
        let values = self.collect_current_values().await;
        self.watchers.check(&values).await;

        Ok(())
    }

    /// Get a setting value (panics if no context set)
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let ctx = self.get_effective_context();
        self.get_internal(key, &ctx)
    }

    /// Get a setting value with default
    pub fn get_or<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.get(key).unwrap_or(default)
    }

    /// Create a getter function
    pub fn getter<T: DeserializeOwned + Clone + 'static>(
        &self,
        key: &str,
        default: T,
    ) -> impl Fn() -> T {
        let key = key.to_string();
        let default = default.clone();
        // Note: This is a simplified version - in production you'd want Arc<RuntimeSettings>
        move || default.clone()
    }

    /// Add a watcher for a key
    pub fn add_watcher(&self, key: &str, watcher: Watcher) -> WatcherId {
        self.watchers.add(key, watcher)
    }

    /// Remove a watcher
    pub fn remove_watcher(&self, id: WatcherId) {
        self.watchers.remove(id)
    }

    /// Set thread-local context
    pub fn set_context(&self, ctx: Context) -> ContextGuard {
        set_thread_context(ctx)
    }

    /// Set thread-local request
    pub fn set_request(&self, req: Request) -> RequestGuard {
        set_thread_request(req)
    }

    /// Execute async closure with task-local context
    pub async fn with_context<F, T>(&self, ctx: Context, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        with_task_context(ctx, f).await
    }

    /// Execute async closure with task-local request
    pub async fn with_request<F, T>(&self, req: Request, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        with_task_request(req, f).await
    }

    // Internal helpers

    fn get_effective_context(&self) -> Context {
        let mut ctx = current_context().expect("Context not set - call set_context() first");

        // Merge with static context
        if ctx.application.is_empty() {
            ctx.application = self.static_context.application.clone();
        }
        if ctx.server.is_empty() {
            ctx.server = self.static_context.server.clone();
        }
        if ctx.mcs_run_env.is_none() {
            ctx.mcs_run_env = self.static_context.mcs_run_env.clone();
        }
        ctx.environment.extend(self.static_context.environment.clone());
        ctx.libraries_versions.extend(self.static_context.libraries_versions.clone());

        // Merge request if available
        if ctx.request.is_none() {
            ctx.request = current_request();
        }

        ctx
    }

    fn get_internal<T: DeserializeOwned>(&self, key: &str, ctx: &Context) -> Option<T> {
        // Use blocking read for sync get
        let state = futures::executor::block_on(self.state.read());

        let settings = state.settings.get(key)?;
        let static_ctx = StaticContext::from(ctx);

        for setting in settings {
            // Check static filters
            if !check_static_filters(&setting.filter, &static_ctx) {
                continue;
            }

            // Check dynamic filters
            if !check_dynamic_filters(&setting.filter, ctx) {
                continue;
            }

            // Found matching setting - deserialize
            match serde_json::from_value(setting.value.clone()) {
                Ok(v) => return Some(v),
                Err(e) => {
                    tracing::warn!("Failed to deserialize setting '{}': {}", key, e);
                    return None;
                }
            }
        }

        None
    }

    async fn merge_settings(&self, response: ProviderResponse) {
        let mut state = self.state.write().await;

        // Update version
        if !response.version.is_empty() {
            state.version = response.version;
        }

        // Delete settings
        for key in response.deleted {
            if let Some(settings) = state.settings.get_mut(&key.key) {
                settings.retain(|s| s.priority != key.priority);
            }
        }

        // Add/update settings
        for setting in response.settings {
            let key = setting.key.clone();
            let priority = setting.priority;

            let settings = state.settings.entry(key).or_insert_with(Vec::new);

            // Remove existing with same priority
            settings.retain(|s| s.priority != priority);

            // Add new
            settings.push(setting);

            // Sort by priority descending
            settings.sort_by(|a, b| b.priority.cmp(&a.priority));
        }
    }

    async fn collect_current_values(&self) -> HashMap<String, serde_json::Value> {
        let state = self.state.read().await;
        state.settings
            .iter()
            .filter_map(|(key, settings)| {
                settings.first().map(|s| (key.clone(), s.value.clone()))
            })
            .collect()
    }
}

/// Builder for RuntimeSettings
pub struct RuntimeSettingsBuilder {
    application: String,
    server: String,
    environment: HashMap<String, String>,
    libraries_versions: HashMap<String, Version>,
    mcs_run_env: Option<String>,
    mcs_enabled: bool,
    mcs_base_url: Option<String>,
    file_path: Option<String>,
    env_enabled: bool,
}

impl RuntimeSettingsBuilder {
    pub fn new() -> Self {
        Self {
            application: String::new(),
            server: gethostname::gethostname().to_string_lossy().to_string(),
            environment: std::env::vars().collect(),
            libraries_versions: HashMap::new(),
            mcs_run_env: std::env::var("MCS_RUN_ENV").ok(),
            mcs_enabled: true,
            mcs_base_url: None,
            file_path: None,
            env_enabled: true,
        }
    }

    pub fn application(mut self, name: impl Into<String>) -> Self {
        self.application = name.into();
        self
    }

    pub fn server(mut self, name: impl Into<String>) -> Self {
        self.server = name.into();
        self
    }

    pub fn library_version(mut self, name: impl Into<String>, version: impl AsRef<str>) -> Self {
        if let Ok(v) = Version::parse(version.as_ref()) {
            self.libraries_versions.insert(name.into(), v);
        }
        self
    }

    pub fn mcs_enabled(mut self, enabled: bool) -> Self {
        self.mcs_enabled = enabled;
        self
    }

    pub fn mcs_base_url(mut self, url: impl Into<String>) -> Self {
        self.mcs_base_url = Some(url.into());
        self
    }

    pub fn file_path(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    pub fn env_enabled(mut self, enabled: bool) -> Self {
        self.env_enabled = enabled;
        self
    }

    pub fn build(self) -> Result<RuntimeSettings, SettingsError> {
        let mut providers: Vec<Box<dyn SettingsProvider>> = Vec::new();

        // Add env provider (lowest priority)
        if self.env_enabled {
            providers.push(Box::new(EnvProvider::from_env()));
        }

        // Add file provider
        if let Some(path) = self.file_path {
            providers.push(Box::new(FileProvider::new(path.into())));
        } else {
            providers.push(Box::new(FileProvider::from_env()));
        }

        // Add MCS provider (highest priority for remote settings)
        if self.mcs_enabled {
            let base_url = self.mcs_base_url.unwrap_or_else(|| {
                std::env::var("RUNTIME_SETTINGS_BASE_URL")
                    .unwrap_or_else(|_| "http://master.runtime-settings.dev3.cian.ru".to_string())
            });
            providers.push(Box::new(McsProvider::new(
                base_url,
                self.application.clone(),
                self.mcs_run_env.clone(),
            )));
        }

        let static_context = StaticContext {
            application: self.application,
            server: self.server,
            environment: self.environment,
            libraries_versions: self.libraries_versions,
            mcs_run_env: self.mcs_run_env,
        };

        let secrets = SecretsService::from_env()?;

        Ok(RuntimeSettings {
            providers,
            state: RwLock::new(SettingsState::default()),
            secrets,
            watchers: WatchersService::new(),
            static_context,
        })
    }
}

impl Default for RuntimeSettingsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ... tests at the bottom
```

**Step 3: Add gethostname to dependencies**

```toml
# Add to lib/runtime-settings/Cargo.toml [dependencies]
gethostname = "0.4"
```

**Step 4: Update lib.rs**

```rust
// lib/runtime-settings/src/lib.rs - final version
pub mod context;
pub mod entities;
pub mod error;
pub mod filters;
pub mod providers;
pub mod scoped;
pub mod secrets;
pub mod settings;
pub mod watchers;

pub use context::{Context, Request, StaticContext};
pub use entities::{McsResponse, Setting, SettingKey};
pub use error::SettingsError;
pub use filters::{check_dynamic_filters, check_static_filters, FilterResult};
pub use providers::{EnvProvider, FileProvider, McsProvider, ProviderResponse, SettingsProvider};
pub use scoped::{
    current_context, current_request, set_thread_context, set_thread_request,
    with_task_context, with_task_request, ContextGuard, RequestGuard,
};
pub use secrets::{resolve_secrets, SecretsService};
pub use settings::{RuntimeSettings, RuntimeSettingsBuilder};
pub use watchers::{Watcher, WatcherId, WatchersService};
```

**Step 5: Run tests**

Run: `cargo test -p runtime-settings`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "Implement RuntimeSettings with builder pattern"
```

---

### Task 7.2: Add global setup function

**Files:**
- Create: `lib/runtime-settings/src/setup.rs`
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Implement setup**

```rust
// lib/runtime-settings/src/setup.rs
use crate::settings::RuntimeSettings;
use lazy_static::lazy_static;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::sleep;

static SETTINGS: OnceLock<RuntimeSettings> = OnceLock::new();

/// Get the global settings instance
pub fn settings() -> &'static RuntimeSettings {
    SETTINGS.get().expect("RuntimeSettings not initialized - call setup() first")
}

/// Initialize global settings with builder
pub async fn setup(builder: crate::settings::RuntimeSettingsBuilder) -> Result<(), crate::error::SettingsError> {
    let settings = builder.build()?;
    settings.init().await?;

    SETTINGS.set(settings).map_err(|_| {
        crate::error::SettingsError::Vault("Settings already initialized".to_string())
    })?;

    // Start background refresh
    tokio::spawn(async {
        loop {
            sleep(Duration::from_secs(30)).await;
            if let Err(e) = settings().refresh().await {
                tracing::error!("Settings refresh failed: {}", e);
            }
        }
    });

    Ok(())
}

/// Initialize with default builder (requires RUNTIME_SETTINGS_APPLICATION env var)
pub async fn setup_from_env() -> Result<(), crate::error::SettingsError> {
    let application = std::env::var("RUNTIME_SETTINGS_APPLICATION")
        .unwrap_or_else(|_| "unknown".to_string());

    setup(RuntimeSettings::builder().application(application)).await
}
```

**Step 2: Update lib.rs**

```rust
// Add to lib.rs
pub mod setup;

pub use setup::{settings, setup, setup_from_env};
```

**Step 3: Verify it compiles**

Run: `cargo check -p runtime-settings`
Expected: Success

**Step 4: Commit**

```bash
git add -A && git commit -m "Add global setup function"
```

---

## Phase 8: Integration and Cleanup

### Task 8.1: Update example application

**Files:**
- Modify: `example/src/main.rs`
- Modify: `example/settings.json`

**Step 1: Update example main.rs**

```rust
// example/src/main.rs
use axum::{routing::get, Router};
use runtime_settings::{Context, Request, RuntimeSettings};
use std::collections::HashMap;
use std::net::SocketAddr;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opt {
    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(StructOpt)]
enum Command {
    Serve {
        #[structopt(long, default_value = "127.0.0.1")]
        address: String,
        #[structopt(long, default_value = "8080")]
        port: u16,
    },
}

#[tokio::main]
async fn main() {
    // Initialize logging
    struct_log::setup();

    let opt = Opt::from_args();

    match opt.cmd {
        Command::Serve { address, port } => {
            // Initialize settings
            let settings = RuntimeSettings::builder()
                .application("example-service")
                .mcs_enabled(false)  // Disable MCS for local testing
                .build()
                .expect("Failed to build settings");

            settings.init().await.expect("Failed to init settings");

            // Test getting a setting
            let ctx = Context {
                application: "example-service".to_string(),
                ..Default::default()
            };
            let _guard = settings.set_context(ctx);

            let some_key: Option<String> = settings.get("SOME_KEY");
            tracing::info!(key = "SOME_KEY", value = ?some_key, "Got setting");

            // Start server
            let app = Router::new().route("/", get(|| async { "Hello, World!" }));

            let addr: SocketAddr = format!("{}:{}", address, port).parse().unwrap();
            tracing::info!("Starting server on {}", addr);

            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        }
    }
}
```

**Step 2: Update example settings.json**

```json
[
  {
    "key": "SOME_KEY",
    "priority": 100,
    "filter": {
      "application": "example-service"
    },
    "value": "hello from settings"
  },
  {
    "key": "FEATURE_FLAG",
    "priority": 100,
    "value": true
  },
  {
    "key": "DATABASE_CONFIG",
    "priority": 100,
    "filter": {
      "application": "example-service"
    },
    "value": {
      "host": "localhost",
      "port": 5432,
      "database": "example_db"
    }
  }
]
```

**Step 3: Run example**

Run: `cargo run -p example -- serve`
Expected: Server starts, logs "Got setting" with value

**Step 4: Commit**

```bash
git add -A && git commit -m "Update example application for v2"
```

---

### Task 8.2: Run all tests and fix issues

**Step 1: Run all tests**

Run: `cargo test -p runtime-settings`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -p runtime-settings -- -D warnings`
Expected: No warnings

**Step 3: Format code**

Run: `cargo fmt -p runtime-settings`

**Step 4: Final commit**

```bash
git add -A && git commit -m "Fix clippy warnings and format code"
```

---

### Task 8.3: Update README

**Files:**
- Modify: `lib/runtime-settings/README.md`

**Step 1: Write README**

```markdown
# runtime-settings

Runtime configuration library for Rust microservices with context-aware settings filtering.

## Features

- **Multiple providers**: Environment variables, JSON files, MCS (Microservice Configuration Service)
- **12 filter types**: application, server, environment, mcs_run_env, library_version, url-path, host, email, ip, header, context, probability
- **Vault integration**: Secure secrets management with automatic renewal
- **Watchers**: Subscribe to configuration changes
- **Scoped contexts**: Thread-local and task-local context management

## Quick Start

```rust
use runtime_settings::{RuntimeSettings, Context};

#[tokio::main]
async fn main() {
    // Build and initialize
    let settings = RuntimeSettings::builder()
        .application("my-service")
        .build()
        .unwrap();

    settings.init().await.unwrap();

    // Set context (required before get)
    let ctx = Context {
        application: "my-service".to_string(),
        ..Default::default()
    };
    let _guard = settings.set_context(ctx);

    // Get values
    let feature_flag: bool = settings.get_or("FEATURE_FLAG", false);
    let db_url: Option<String> = settings.get("DATABASE_URL");
}
```

## Configuration

### Environment Variables

- `RUNTIME_SETTINGS_FILE_PATH` - Path to JSON settings file (default: `runtime-settings.json`)
- `RUNTIME_SETTINGS_BASE_URL` - MCS base URL
- `MCS_RUN_ENV` - Environment name for filtering (PROD, STAGING, etc.)
- `VAULT_ADDR` - Vault server address
- `VAULT_TOKEN` - Vault authentication token

### Settings File Format

```json
[
  {
    "key": "DATABASE_URL",
    "priority": 100,
    "filter": {
      "application": "my-service",
      "mcs_run_env": "PROD"
    },
    "value": "postgres://localhost/mydb"
  }
]
```

## Filters

| Filter | Type | Description |
|--------|------|-------------|
| application | static | Regex against application name |
| server | static | Regex against server hostname |
| environment | static | Key=value pairs against env vars |
| mcs_run_env | static | Regex against MCS_RUN_ENV |
| library_version | static | Semver constraints |
| url-path | dynamic | Regex against request path |
| host | dynamic | Regex against Host header |
| email | dynamic | Regex against X-Real-Email header |
| ip | dynamic | Regex against X-Real-IP header |
| header | dynamic | Key=value pairs against headers |
| context | dynamic | Key=value pairs against custom context |
| probability | dynamic | Random percentage (0-100) |
```

**Step 2: Commit**

```bash
git add -A && git commit -m "Update README with v2 documentation"
```

---

## Summary

This plan implements runtime-settings v2 with:

1. **Core infrastructure**: entities, error types, context
2. **12 filters**: 5 static + 7 dynamic
3. **3 providers**: env, file, MCS
4. **Scoped contexts**: thread-local + task-local
5. **Vault integration**: secrets resolution
6. **Watchers**: change notifications
7. **RuntimeSettings**: main API with builder pattern

Total: ~40 steps across 8 phases.
