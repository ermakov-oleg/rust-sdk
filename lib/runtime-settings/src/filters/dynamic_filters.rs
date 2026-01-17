// lib/runtime-settings/src/filters/dynamic_filters.rs
use super::{DynamicFilter, FilterResult};
use crate::context::Context;
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
        Err(e) => {
            tracing::warn!(pattern = %pattern, error = %e, "Failed to compile regex pattern");
            FilterResult::NoMatch
        }
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

pub struct HeaderFilter;
pub struct ContextFilter;
pub struct ProbabilityFilter;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Request;
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
