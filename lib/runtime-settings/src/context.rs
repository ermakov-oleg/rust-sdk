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

/// Hierarchical custom context with eager merge (each snapshot is a complete merged state)
#[derive(Debug, Clone, Default)]
pub struct CustomContext {
    snapshots: Vec<HashMap<String, String>>,
}

impl CustomContext {
    /// Create empty custom context
    pub fn new() -> Self {
        Self { snapshots: vec![] }
    }

    /// Add a new layer on top (merges with current state, new values take priority)
    pub fn push_layer(&mut self, mut layer: HashMap<String, String>) {
        if let Some(current) = self.snapshots.last() {
            // Add old values only if key doesn't exist in new layer
            for (k, v) in current {
                layer.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        self.snapshots.push(layer);
    }

    /// Remove the top layer
    pub fn pop_layer(&mut self) {
        self.snapshots.pop();
    }

    /// Get value by key - O(1) lookup
    pub fn get(&self, key: &str) -> Option<&str> {
        self.snapshots.last()?.get(key).map(|s| s.as_str())
    }

    /// Iterate over all key-value pairs
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.snapshots
            .last()
            .into_iter()
            .flat_map(|m| m.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    }

    /// Check if context is empty
    pub fn is_empty(&self) -> bool {
        self.snapshots.last().is_none_or(|m| m.is_empty())
    }

    /// Get reference to current merged HashMap (for filters)
    pub fn as_map(&self) -> Option<&HashMap<String, String>> {
        self.snapshots.last()
    }
}

/// Context for dynamic filter evaluation
#[derive(Debug, Clone, Default)]
pub struct DynamicContext {
    pub request: Option<Request>,
    pub custom: CustomContext,
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
    fn test_custom_context_single_layer() {
        let mut ctx = CustomContext::new();
        ctx.push_layer([("key1".to_string(), "value1".to_string())].into());
        assert_eq!(ctx.get("key1"), Some("value1"));
        assert_eq!(ctx.get("missing"), None);
    }

    #[test]
    fn test_custom_context_layered_override() {
        let mut ctx = CustomContext::new();
        ctx.push_layer([("key1".to_string(), "base".to_string())].into());
        ctx.push_layer([("key1".to_string(), "override".to_string())].into());
        assert_eq!(ctx.get("key1"), Some("override"));
        ctx.pop_layer();
        assert_eq!(ctx.get("key1"), Some("base"));
    }

    #[test]
    fn test_custom_context_iter() {
        let mut ctx = CustomContext::new();
        ctx.push_layer([("a".to_string(), "1".to_string())].into());
        ctx.push_layer(
            [
                ("b".to_string(), "2".to_string()),
                ("a".to_string(), "override".to_string()),
            ]
            .into(),
        );
        let items: HashMap<&str, &str> = ctx.iter().collect();
        assert_eq!(items.get("a"), Some(&"override"));
        assert_eq!(items.get("b"), Some(&"2"));
    }

    #[test]
    fn test_dynamic_context_default() {
        let ctx = DynamicContext::default();
        assert!(ctx.request.is_none());
        assert!(ctx.custom.is_empty());
    }
}
