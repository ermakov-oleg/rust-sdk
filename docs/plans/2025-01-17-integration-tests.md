# Integration Tests Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add integration tests for filters, MCS provider, and Vault secrets using HTTP mocking.

**Architecture:** Use `wiremock` crate to mock HTTP responses for MCS and Vault. Filter tests will test the full pipeline from RawSetting to Setting with context matching.

**Tech Stack:** wiremock 0.6, tokio-test, serde_json

---

## Task 1: Add wiremock dev-dependency

**Files:**
- Modify: `lib/runtime-settings/Cargo.toml`

**Step 1: Add wiremock to dev-dependencies**

```toml
[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
tempfile = "3"
wiremock = "0.6"
```

**Step 2: Verify it compiles**

Run: `cargo check -p runtime-settings`
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add lib/runtime-settings/Cargo.toml
git commit -m "Add wiremock dev-dependency for integration tests"
```

---

## Task 2: Create integration_filters.rs - test setup

**Files:**
- Create: `lib/runtime-settings/tests/integration_filters.rs`

**Step 1: Create test file with imports and helper**

```rust
//! Integration tests for filter compilation and matching pipeline.
//!
//! Tests the full flow: RawSetting -> Setting (compiled) -> context matching

use runtime_settings::context::{Context, Request, StaticContext};
use runtime_settings::entities::{RawSetting, Setting};
use std::collections::HashMap;

/// Helper to create a RawSetting with filters
fn raw_setting(key: &str, priority: i64, filters: &[(&str, &str)], value: serde_json::Value) -> RawSetting {
    RawSetting {
        key: key.to_string(),
        priority,
        filter: filters.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        value,
    }
}

/// Helper to create StaticContext
fn static_ctx(app: &str, server: &str, mcs_run_env: Option<&str>) -> StaticContext {
    StaticContext {
        application: app.to_string(),
        server: server.to_string(),
        environment: HashMap::new(),
        libraries_versions: HashMap::new(),
        mcs_run_env: mcs_run_env.map(|s| s.to_string()),
    }
}

/// Helper to create Context with request
fn request_ctx(path: &str, email: Option<&str>, ip: Option<&str>) -> Context {
    let mut headers = HashMap::new();
    if let Some(e) = email {
        headers.insert("x-real-email".to_string(), e.to_string());
    }
    if let Some(i) = ip {
        headers.insert("x-real-ip".to_string(), i.to_string());
    }
    Context {
        request: Some(Request {
            method: "GET".to_string(),
            path: path.to_string(),
            headers,
        }),
        ..Default::default()
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo test -p runtime-settings --test integration_filters --no-run`
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add lib/runtime-settings/tests/integration_filters.rs
git commit -m "Add integration_filters.rs test setup"
```

---

## Task 3: Add static filter integration tests

**Files:**
- Modify: `lib/runtime-settings/tests/integration_filters.rs`

**Step 1: Add test for application filter matching**

```rust
#[test]
fn test_application_filter_exact_match() {
    let raw = raw_setting("KEY", 100, &[("application", "my-app")], serde_json::json!("value"));
    let setting = Setting::compile(raw).unwrap();

    assert!(setting.check_static_filters(&static_ctx("my-app", "server1", None)));
    assert!(!setting.check_static_filters(&static_ctx("other-app", "server1", None)));
}

#[test]
fn test_application_filter_regex() {
    let raw = raw_setting("KEY", 100, &[("application", "my-.*")], serde_json::json!("value"));
    let setting = Setting::compile(raw).unwrap();

    assert!(setting.check_static_filters(&static_ctx("my-app", "server1", None)));
    assert!(setting.check_static_filters(&static_ctx("my-service", "server1", None)));
    assert!(!setting.check_static_filters(&static_ctx("other-app", "server1", None)));
}
```

**Step 2: Add test for mcs_run_env filter (Python compatibility)**

```rust
#[test]
fn test_mcs_run_env_filter_returns_false_when_none() {
    // This is the Python behavior fix - should return false, not NotApplicable
    let raw = raw_setting("KEY", 100, &[("mcs_run_env", "PROD")], serde_json::json!("value"));
    let setting = Setting::compile(raw).unwrap();

    // When mcs_run_env is None, filter should return false
    assert!(!setting.check_static_filters(&static_ctx("app", "server", None)));

    // When mcs_run_env matches, filter should return true
    assert!(setting.check_static_filters(&static_ctx("app", "server", Some("PROD"))));

    // When mcs_run_env doesn't match, filter should return false
    assert!(!setting.check_static_filters(&static_ctx("app", "server", Some("DEV"))));
}

#[test]
fn test_multiple_static_filters_all_must_match() {
    let raw = raw_setting(
        "KEY", 100,
        &[("application", "my-app"), ("server", "server-1")],
        serde_json::json!("value")
    );
    let setting = Setting::compile(raw).unwrap();

    // Both filters match
    assert!(setting.check_static_filters(&static_ctx("my-app", "server-1", None)));

    // Only application matches
    assert!(!setting.check_static_filters(&static_ctx("my-app", "server-2", None)));

    // Only server matches
    assert!(!setting.check_static_filters(&static_ctx("other-app", "server-1", None)));
}
```

**Step 3: Run tests**

Run: `cargo test -p runtime-settings --test integration_filters`
Expected: All tests pass

**Step 4: Commit**

```bash
git add lib/runtime-settings/tests/integration_filters.rs
git commit -m "Add static filter integration tests"
```

---

## Task 4: Add dynamic filter integration tests

**Files:**
- Modify: `lib/runtime-settings/tests/integration_filters.rs`

**Step 1: Add tests for dynamic filters**

```rust
#[test]
fn test_url_path_filter() {
    let raw = raw_setting("KEY", 100, &[("url-path", "/api/.*")], serde_json::json!("value"));
    let setting = Setting::compile(raw).unwrap();

    assert!(setting.check_dynamic_filters(&request_ctx("/api/users", None, None)));
    assert!(setting.check_dynamic_filters(&request_ctx("/api/orders/123", None, None)));
    assert!(!setting.check_dynamic_filters(&request_ctx("/web/page", None, None)));
}

#[test]
fn test_email_filter() {
    let raw = raw_setting("KEY", 100, &[("email", ".*@example\\.com")], serde_json::json!("value"));
    let setting = Setting::compile(raw).unwrap();

    assert!(setting.check_dynamic_filters(&request_ctx("/", Some("user@example.com"), None)));
    assert!(setting.check_dynamic_filters(&request_ctx("/", Some("admin@example.com"), None)));
    assert!(!setting.check_dynamic_filters(&request_ctx("/", Some("user@other.com"), None)));

    // No email header - filter returns false
    assert!(!setting.check_dynamic_filters(&request_ctx("/", None, None)));
}

#[test]
fn test_ip_filter() {
    let raw = raw_setting("KEY", 100, &[("ip", "192\\.168\\..*")], serde_json::json!("value"));
    let setting = Setting::compile(raw).unwrap();

    assert!(setting.check_dynamic_filters(&request_ctx("/", None, Some("192.168.1.1"))));
    assert!(setting.check_dynamic_filters(&request_ctx("/", None, Some("192.168.100.50"))));
    assert!(!setting.check_dynamic_filters(&request_ctx("/", None, Some("10.0.0.1"))));
}

#[test]
fn test_mixed_static_and_dynamic_filters() {
    let raw = raw_setting(
        "KEY", 100,
        &[("application", "my-app"), ("url-path", "/api/.*")],
        serde_json::json!("value")
    );
    let setting = Setting::compile(raw).unwrap();

    let static_ctx = static_ctx("my-app", "server", None);

    // Static matches, dynamic matches
    assert!(setting.check_static_filters(&static_ctx));
    assert!(setting.check_dynamic_filters(&request_ctx("/api/users", None, None)));

    // Static doesn't match
    let wrong_static_ctx = static_ctx("other-app", "server", None);
    assert!(!setting.check_static_filters(&wrong_static_ctx));

    // Static matches but dynamic doesn't
    assert!(!setting.check_dynamic_filters(&request_ctx("/web/page", None, None)));
}
```

**Step 2: Run tests**

Run: `cargo test -p runtime-settings --test integration_filters`
Expected: All tests pass

**Step 3: Commit**

```bash
git add lib/runtime-settings/tests/integration_filters.rs
git commit -m "Add dynamic filter integration tests"
```

---

## Task 5: Create integration_mcs.rs - MCS provider tests

**Files:**
- Create: `lib/runtime-settings/tests/integration_mcs.rs`

**Step 1: Create test file with MCS mock setup**

```rust
//! Integration tests for MCS provider with HTTP mocking

use runtime_settings::entities::RawSetting;
use runtime_settings::providers::{McsProvider, SettingsProvider};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_mcs_provider_loads_settings() {
    let mock_server = MockServer::start().await;

    let response_body = serde_json::json!({
        "settings": [
            {"key": "TEST_KEY", "priority": 100, "filter": {}, "value": "test-value"},
            {"key": "OTHER_KEY", "priority": 50, "filter": {"application": "my-app"}, "value": 123}
        ],
        "deleted": [],
        "version": "42"
    });

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .and(query_param("runtime", "rust"))
        .and(query_param("version", "0"))
        .and(query_param("application", "test-app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(mock_server.uri(), "test-app".to_string(), None);
    let response = provider.load("0").await.unwrap();

    assert_eq!(response.settings.len(), 2);
    assert_eq!(response.settings[0].key, "TEST_KEY");
    assert_eq!(response.settings[0].value, serde_json::json!("test-value"));
    assert_eq!(response.version, "42");
}

#[tokio::test]
async fn test_mcs_provider_handles_deleted_keys() {
    let mock_server = MockServer::start().await;

    let response_body = serde_json::json!({
        "settings": [],
        "deleted": [
            {"key": "OLD_KEY", "priority": 100}
        ],
        "version": "43"
    });

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(mock_server.uri(), "test-app".to_string(), None);
    let response = provider.load("42").await.unwrap();

    assert!(response.settings.is_empty());
    assert_eq!(response.deleted.len(), 1);
    assert_eq!(response.deleted[0].key, "OLD_KEY");
}

#[tokio::test]
async fn test_mcs_provider_includes_mcs_run_env() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .and(query_param("mcs_run_env", "PROD"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "settings": [],
            "deleted": [],
            "version": "1"
        })))
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(mock_server.uri(), "test-app".to_string(), Some("PROD".to_string()));
    let response = provider.load("0").await.unwrap();

    assert_eq!(response.version, "1");
}

#[tokio::test]
async fn test_mcs_provider_handles_error_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(mock_server.uri(), "test-app".to_string(), None);
    let result = provider.load("0").await;

    assert!(result.is_err());
}
```

**Step 2: Run tests**

Run: `cargo test -p runtime-settings --test integration_mcs`
Expected: All tests pass

**Step 3: Commit**

```bash
git add lib/runtime-settings/tests/integration_mcs.rs
git commit -m "Add MCS provider integration tests"
```

---

## Task 6: Create integration_vault.rs - Vault secrets tests

**Files:**
- Create: `lib/runtime-settings/tests/integration_vault.rs`

**Step 1: Create test file with Vault mock setup**

```rust
//! Integration tests for Vault secrets with HTTP mocking
//!
//! Note: We test the SecretsService directly using wiremock to mock Vault HTTP API.
//! The vaultrs library makes HTTP calls to Vault, which we intercept.

use runtime_settings::secrets::SecretsService;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};

/// Helper to create VaultClient pointing to mock server
fn mock_vault_client(mock_uri: &str, token: &str) -> VaultClient {
    let settings = VaultClientSettingsBuilder::default()
        .address(mock_uri)
        .token(token)
        .build()
        .unwrap();
    VaultClient::new(settings).unwrap()
}

#[tokio::test]
async fn test_vault_get_secret() {
    let mock_server = MockServer::start().await;

    // Vault KV2 read endpoint returns data nested in data.data
    let response_body = serde_json::json!({
        "data": {
            "data": {
                "username": "db_user",
                "password": "secret123"
            },
            "metadata": {
                "version": 1
            }
        }
    });

    Mock::given(method("GET"))
        .and(path("/v1/secret/data/database/credentials"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&mock_server)
        .await;

    let client = mock_vault_client(&mock_server.uri(), "test-token");
    let secrets = SecretsService::new(client);

    let password = secrets.get("database/credentials", "password").await.unwrap();
    assert_eq!(password, serde_json::json!("secret123"));

    let username = secrets.get("database/credentials", "username").await.unwrap();
    assert_eq!(username, serde_json::json!("db_user"));
}

#[tokio::test]
async fn test_vault_secret_caching() {
    let mock_server = MockServer::start().await;

    let response_body = serde_json::json!({
        "data": {
            "data": {"key": "cached_value"},
            "metadata": {"version": 1}
        }
    });

    Mock::given(method("GET"))
        .and(path("/v1/secret/data/test/path"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1) // Should only be called once due to caching
        .mount(&mock_server)
        .await;

    let client = mock_vault_client(&mock_server.uri(), "test-token");
    let secrets = SecretsService::new(client);

    // First call - hits Vault
    let value1 = secrets.get("test/path", "key").await.unwrap();
    assert_eq!(value1, serde_json::json!("cached_value"));

    // Second call - should use cache
    let value2 = secrets.get("test/path", "key").await.unwrap();
    assert_eq!(value2, serde_json::json!("cached_value"));
}

#[tokio::test]
async fn test_vault_secret_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/secret/data/nonexistent"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let client = mock_vault_client(&mock_server.uri(), "test-token");
    let secrets = SecretsService::new(client);

    let result = secrets.get("nonexistent", "key").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_vault_key_not_found_in_secret() {
    let mock_server = MockServer::start().await;

    let response_body = serde_json::json!({
        "data": {
            "data": {"existing_key": "value"},
            "metadata": {"version": 1}
        }
    });

    Mock::given(method("GET"))
        .and(path("/v1/secret/data/test/secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&mock_server)
        .await;

    let client = mock_vault_client(&mock_server.uri(), "test-token");
    let secrets = SecretsService::new(client);

    let result = secrets.get("test/secret", "nonexistent_key").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_secrets_without_vault() {
    let secrets = SecretsService::new_without_vault();

    let result = secrets.get("any/path", "key").await;
    assert!(result.is_err());
}
```

**Step 2: Run tests**

Run: `cargo test -p runtime-settings --test integration_vault`
Expected: All tests pass

**Step 3: Commit**

```bash
git add lib/runtime-settings/tests/integration_vault.rs
git commit -m "Add Vault secrets integration tests"
```

---

## Task 7: Final verification

**Step 1: Run all integration tests**

Run: `cargo test -p runtime-settings --test '*'`
Expected: All integration tests pass

**Step 2: Run full test suite**

Run: `cargo test -p runtime-settings`
Expected: All 150+ tests pass

**Step 3: Run clippy**

Run: `cargo clippy -p runtime-settings -- -D warnings`
Expected: No warnings

**Step 4: Commit any remaining changes**

```bash
git status
# If clean, no commit needed
```
