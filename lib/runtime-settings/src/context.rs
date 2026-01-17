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

/// Hierarchical custom context with ChainMap-like semantics
#[derive(Debug, Clone, Default)]
pub struct CustomContext {
    layers: Vec<HashMap<String, String>>,
}

impl CustomContext {
    /// Create empty custom context
    pub fn new() -> Self {
        Self { layers: vec![] }
    }

    /// Add a new layer on top
    pub fn push_layer(&mut self, layer: HashMap<String, String>) {
        self.layers.push(layer);
    }

    /// Remove the top layer
    pub fn pop_layer(&mut self) {
        self.layers.pop();
    }

    /// Get value by key (searches from top layer to bottom)
    pub fn get(&self, key: &str) -> Option<&str> {
        for layer in self.layers.iter().rev() {
            if let Some(v) = layer.get(key) {
                return Some(v.as_str());
            }
        }
        None
    }

    /// Iterate over all unique key-value pairs (top layer wins)
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for layer in self.layers.iter().rev() {
            for (k, v) in layer {
                if seen.insert(k.as_str()) {
                    result.push((k.as_str(), v.as_str()));
                }
            }
        }
        result.into_iter()
    }

    /// Check if context is empty
    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|l| l.is_empty())
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
