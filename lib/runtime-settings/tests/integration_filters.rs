//! Integration tests for the filter compilation and matching pipeline.
//!
//! This module tests the full flow from raw settings (RawSetting) to compiled
//! settings (Setting) to context matching. It verifies that filters are correctly
//! compiled and that matching logic works as expected across the full pipeline.

use runtime_settings::context::{CustomContext, DynamicContext, Request, StaticContext};
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

/// Create a DynamicContext with request information for testing purposes.
///
/// # Arguments
/// * `path` - URL path for the request
/// * `email` - Optional email (set via x-real-email header)
/// * `ip` - Optional IP address (set via x-real-ip header)
#[allow(dead_code)]
fn request_ctx(path: &str, email: Option<&str>, ip: Option<&str>) -> DynamicContext {
    let mut headers = HashMap::new();
    if let Some(e) = email {
        headers.insert("x-real-email".to_string(), e.to_string());
    }
    if let Some(i) = ip {
        headers.insert("x-real-ip".to_string(), i.to_string());
    }

    DynamicContext {
        request: Some(Request {
            method: "GET".to_string(),
            path: path.to_string(),
            headers,
        }),
        custom: CustomContext::new(),
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

    #[test]
    fn test_application_filter_exact_match() {
        // Create setting with application filter
        let raw = raw_setting(
            "APP_SETTING",
            100,
            &[("application", "my-app")],
            serde_json::json!("value"),
        );
        let setting = Setting::compile(raw).expect("should compile");

        // Should match when application is "my-app"
        let ctx_match = static_ctx("my-app", "server1", None);
        assert!(
            setting.check_static_filters(&ctx_match),
            "Should match when application is 'my-app'"
        );

        // Should NOT match when application is different
        let ctx_no_match = static_ctx("other-app", "server1", None);
        assert!(
            !setting.check_static_filters(&ctx_no_match),
            "Should not match when application is 'other-app'"
        );
    }

    #[test]
    fn test_mcs_run_env_filter_returns_false_when_none() {
        // This test verifies the Python compatibility fix:
        // When mcs_run_env filter is set but context has None, it should return FALSE
        // (not NotApplicable which would be treated as a match)
        let raw = raw_setting(
            "ENV_SETTING",
            100,
            &[("mcs_run_env", "PROD")],
            serde_json::json!("value"),
        );
        let setting = Setting::compile(raw).expect("should compile");

        // Should return FALSE when mcs_run_env is None (the fix)
        let ctx_none = static_ctx("my-app", "server1", None);
        assert!(
            !setting.check_static_filters(&ctx_none),
            "Should return FALSE when mcs_run_env is None"
        );

        // Should return TRUE when mcs_run_env matches "PROD"
        let ctx_prod = static_ctx("my-app", "server1", Some("PROD"));
        assert!(
            setting.check_static_filters(&ctx_prod),
            "Should return TRUE when mcs_run_env matches 'PROD'"
        );

        // Should return FALSE when mcs_run_env is "DEV" (doesn't match)
        let ctx_dev = static_ctx("my-app", "server1", Some("DEV"));
        assert!(
            !setting.check_static_filters(&ctx_dev),
            "Should return FALSE when mcs_run_env is 'DEV'"
        );
    }

    #[test]
    fn test_multiple_static_filters_all_must_match() {
        // Create setting with multiple static filters
        let raw = raw_setting(
            "MULTI_FILTER_SETTING",
            100,
            &[("application", "my-app"), ("server", "server-1")],
            serde_json::json!("value"),
        );
        let setting = Setting::compile(raw).expect("should compile");

        // Should match when BOTH application and server match
        let ctx_both_match = static_ctx("my-app", "server-1", None);
        assert!(
            setting.check_static_filters(&ctx_both_match),
            "Should match when both application and server match"
        );

        // Should NOT match when only application matches
        let ctx_app_only = static_ctx("my-app", "other-server", None);
        assert!(
            !setting.check_static_filters(&ctx_app_only),
            "Should not match when only application matches"
        );

        // Should NOT match when only server matches
        let ctx_server_only = static_ctx("other-app", "server-1", None);
        assert!(
            !setting.check_static_filters(&ctx_server_only),
            "Should not match when only server matches"
        );
    }

    // ==================== Dynamic Filter Tests ====================

    #[test]
    fn test_url_path_filter() {
        // Create setting with url-path filter (regex pattern)
        let raw = raw_setting(
            "URL_PATH_SETTING",
            100,
            &[("url-path", "/api/.*")],
            serde_json::json!("api_value"),
        );
        let setting = Setting::compile(raw).expect("should compile");

        // Should match /api/users
        let ctx_users = request_ctx("/api/users", None, None);
        assert!(
            setting.check_dynamic_filters(&ctx_users),
            "Should match /api/users"
        );

        // Should match /api/orders/123
        let ctx_orders = request_ctx("/api/orders/123", None, None);
        assert!(
            setting.check_dynamic_filters(&ctx_orders),
            "Should match /api/orders/123"
        );

        // Should NOT match /web/page
        let ctx_web = request_ctx("/web/page", None, None);
        assert!(
            !setting.check_dynamic_filters(&ctx_web),
            "Should not match /web/page"
        );
    }

    #[test]
    fn test_email_filter() {
        // Create setting with email filter (regex pattern, dot escaped)
        let raw = raw_setting(
            "EMAIL_SETTING",
            100,
            &[("email", ".*@example\\.com")],
            serde_json::json!("email_value"),
        );
        let setting = Setting::compile(raw).expect("should compile");

        // Should match user@example.com
        let ctx_user = request_ctx("/", Some("user@example.com"), None);
        assert!(
            setting.check_dynamic_filters(&ctx_user),
            "Should match user@example.com"
        );

        // Should match admin@example.com
        let ctx_admin = request_ctx("/", Some("admin@example.com"), None);
        assert!(
            setting.check_dynamic_filters(&ctx_admin),
            "Should match admin@example.com"
        );

        // Should NOT match user@other.com
        let ctx_other = request_ctx("/", Some("user@other.com"), None);
        assert!(
            !setting.check_dynamic_filters(&ctx_other),
            "Should not match user@other.com"
        );

        // When no email header is present, the filter is NotApplicable and passes (returns true)
        // This is by design - the filter only applies when email header exists
        let ctx_no_email = request_ctx("/", None, None);
        assert!(
            setting.check_dynamic_filters(&ctx_no_email),
            "Should pass (NotApplicable) when no email header is present"
        );
    }

    #[test]
    fn test_ip_filter() {
        // Create setting with ip filter (regex pattern, dots escaped)
        let raw = raw_setting(
            "IP_SETTING",
            100,
            &[("ip", "192\\.168\\..*")],
            serde_json::json!("ip_value"),
        );
        let setting = Setting::compile(raw).expect("should compile");

        // Should match 192.168.1.1
        let ctx_match1 = request_ctx("/", None, Some("192.168.1.1"));
        assert!(
            setting.check_dynamic_filters(&ctx_match1),
            "Should match 192.168.1.1"
        );

        // Should match 192.168.100.50
        let ctx_match2 = request_ctx("/", None, Some("192.168.100.50"));
        assert!(
            setting.check_dynamic_filters(&ctx_match2),
            "Should match 192.168.100.50"
        );

        // Should NOT match 10.0.0.1
        let ctx_no_match = request_ctx("/", None, Some("10.0.0.1"));
        assert!(
            !setting.check_dynamic_filters(&ctx_no_match),
            "Should not match 10.0.0.1"
        );
    }

    #[test]
    fn test_mixed_static_and_dynamic_filters() {
        // Create setting with both static (application) and dynamic (url-path) filters
        let raw = raw_setting(
            "MIXED_FILTER_SETTING",
            100,
            &[("application", "my-app"), ("url-path", "/api/.*")],
            serde_json::json!("mixed_value"),
        );
        let setting = Setting::compile(raw).expect("should compile");

        // Create contexts for testing
        let static_ctx_match = static_ctx("my-app", "server1", None);
        let static_ctx_no_match = static_ctx("other-app", "server1", None);
        let dynamic_ctx_match = request_ctx("/api/users", None, None);
        let dynamic_ctx_no_match = request_ctx("/web/page", None, None);

        // Static filter should match when application is "my-app"
        assert!(
            setting.check_static_filters(&static_ctx_match),
            "Static filter should match when application is 'my-app'"
        );

        // Static filter should NOT match when application is different
        assert!(
            !setting.check_static_filters(&static_ctx_no_match),
            "Static filter should not match when application is 'other-app'"
        );

        // Dynamic filter should match when url-path matches /api/.*
        assert!(
            setting.check_dynamic_filters(&dynamic_ctx_match),
            "Dynamic filter should match when path is /api/users"
        );

        // Dynamic filter should NOT match when url-path doesn't match
        assert!(
            !setting.check_dynamic_filters(&dynamic_ctx_no_match),
            "Dynamic filter should not match when path is /web/page"
        );

        // Both filters work independently - for a setting to fully match,
        // both static AND dynamic filters must pass
    }
}
