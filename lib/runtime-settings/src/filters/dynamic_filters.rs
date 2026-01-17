// lib/runtime-settings/src/filters/dynamic_filters.rs
use super::{DynamicFilter, FilterResult};
use crate::context::Context;
use rand::Rng;
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
}
