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
