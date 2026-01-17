//! Integration tests for the filter compilation and matching pipeline.
//!
//! This module tests the full flow from raw settings (RawSetting) to compiled
//! settings (Setting) to context matching. It verifies that filters are correctly
//! compiled and that matching logic works as expected across the full pipeline.

use runtime_settings::context::{Context, Request, StaticContext};
use runtime_settings::entities::{RawSetting, Setting};
use std::collections::HashMap;

/// Create a RawSetting for testing purposes.
///
/// # Arguments
/// * `key` - The setting key name
/// * `priority` - Priority value (higher = more specific)
/// * `filters` - Slice of (filter_name, pattern) tuples
/// * `value` - The JSON value for the setting
#[allow(dead_code)]
fn raw_setting(
    key: &str,
    priority: i64,
    filters: &[(&str, &str)],
    value: serde_json::Value,
) -> RawSetting {
    let filter: HashMap<String, String> = filters
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    RawSetting {
        key: key.to_string(),
        priority,
        filter,
        value,
    }
}

/// Create a StaticContext for testing purposes.
///
/// # Arguments
/// * `app` - Application name
/// * `server` - Server name
/// * `mcs_run_env` - Optional MCS run environment
#[allow(dead_code)]
fn static_ctx(app: &str, server: &str, mcs_run_env: Option<&str>) -> StaticContext {
    StaticContext {
        application: app.to_string(),
        server: server.to_string(),
        environment: HashMap::new(),
        libraries_versions: HashMap::new(),
        mcs_run_env: mcs_run_env.map(|s| s.to_string()),
    }
}

/// Create a Context with request information for testing purposes.
///
/// # Arguments
/// * `path` - URL path for the request
/// * `email` - Optional email (set via x-real-email header)
/// * `ip` - Optional IP address (set via x-real-ip header)
#[allow(dead_code)]
fn request_ctx(path: &str, email: Option<&str>, ip: Option<&str>) -> Context {
    let mut headers = HashMap::new();
    if let Some(e) = email {
        headers.insert("x-real-email".to_string(), e.to_string());
    }
    if let Some(i) = ip {
        headers.insert("x-real-ip".to_string(), i.to_string());
    }

    Context {
        application: String::new(),
        server: String::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helpers_compile() {
        // Verify that helper functions work and Setting::compile is accessible
        let raw = raw_setting("TEST_KEY", 100, &[("application", "test-app")], serde_json::json!("value"));
        let setting = Setting::compile(raw).expect("should compile");
        assert_eq!(setting.key, "TEST_KEY");
        assert_eq!(setting.priority, 100);

        let static_context = static_ctx("my-app", "server-1", Some("production"));
        assert_eq!(static_context.application, "my-app");
        assert_eq!(static_context.server, "server-1");
        assert_eq!(static_context.mcs_run_env, Some("production".to_string()));

        let ctx = request_ctx("/api/users", Some("user@example.com"), Some("192.168.1.1"));
        assert!(ctx.request.is_some());
        let req = ctx.request.as_ref().unwrap();
        assert_eq!(req.path, "/api/users");
        assert_eq!(req.email(), Some("user@example.com"));
        assert_eq!(req.ip(), Some("192.168.1.1"));
    }
}
