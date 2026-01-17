// lib/runtime-settings/src/filters/dynamic_filters.rs
use super::{CompiledDynamicFilter, DynamicFilter, FilterResult};
use crate::context::DynamicContext;
use crate::error::SettingsError;
use rand::Rng;
use regex::{Regex, RegexBuilder};

/// Helper to check regex pattern against value (case-insensitive, anchored)
fn check_regex(pattern: &str, value: &str) -> FilterResult {
    let anchored = format!("^(?:{})$", pattern);
    match RegexBuilder::new(&anchored).case_insensitive(true).build() {
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

    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult {
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

    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult {
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

    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult {
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

    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult {
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

/// Helper for case-insensitive header lookup
fn check_header_filter(
    pattern: &str,
    headers: &std::collections::HashMap<String, String>,
) -> FilterResult {
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

    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult {
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

    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult {
        match ctx.custom.as_map() {
            Some(map) => check_map_filter(pattern, map),
            None => FilterResult::NoMatch,
        }
    }
}

/// probability: "25" — 25% chance of Match
pub struct ProbabilityFilter;

impl DynamicFilter for ProbabilityFilter {
    fn name(&self) -> &'static str {
        "probability"
    }

    fn check(&self, pattern: &str, _ctx: &DynamicContext) -> FilterResult {
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

// =============================================================================
// Compiled Dynamic Filters (pre-compiled regex for hot path optimization)
// =============================================================================

/// Helper to compile an anchored, case-insensitive regex
fn compile_anchored_regex(pattern: &str) -> Result<Regex, SettingsError> {
    let anchored = format!("^(?:{})$", pattern);
    RegexBuilder::new(&anchored)
        .case_insensitive(true)
        .build()
        .map_err(|e| SettingsError::InvalidRegex {
            pattern: pattern.to_string(),
            error: e.to_string(),
        })
}

/// Compiled url-path filter - holds pre-compiled regex, checks against ctx.request.path
/// Returns true if no request (NotApplicable = pass)
pub struct CompiledUrlPathFilter {
    regex: Regex,
}

impl CompiledUrlPathFilter {
    /// Compile a url-path filter from a pattern
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let regex = compile_anchored_regex(pattern)?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledUrlPathFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        match &ctx.request {
            Some(req) => self.regex.is_match(&req.path),
            None => true, // NotApplicable = pass
        }
    }
}

/// Compiled host filter - holds pre-compiled regex, checks against ctx.request.host()
/// Returns true if no request or no host (NotApplicable = pass)
pub struct CompiledHostFilter {
    regex: Regex,
}

impl CompiledHostFilter {
    /// Compile a host filter from a pattern
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let regex = compile_anchored_regex(pattern)?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledHostFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        match &ctx.request {
            Some(req) => match req.host() {
                Some(host) => self.regex.is_match(host),
                None => true, // NotApplicable = pass
            },
            None => true, // NotApplicable = pass
        }
    }
}

/// Compiled email filter - holds pre-compiled regex, checks against ctx.request.email()
/// Returns true if no request or no email (NotApplicable = pass)
pub struct CompiledEmailFilter {
    regex: Regex,
}

impl CompiledEmailFilter {
    /// Compile an email filter from a pattern
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let regex = compile_anchored_regex(pattern)?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledEmailFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        match &ctx.request {
            Some(req) => match req.email() {
                Some(email) => self.regex.is_match(email),
                None => true, // NotApplicable = pass
            },
            None => true, // NotApplicable = pass
        }
    }
}

/// Compiled ip filter - holds pre-compiled regex, checks against ctx.request.ip()
/// Returns true if no request or no ip (NotApplicable = pass)
pub struct CompiledIpFilter {
    regex: Regex,
}

impl CompiledIpFilter {
    /// Compile an ip filter from a pattern
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let regex = compile_anchored_regex(pattern)?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledIpFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        match &ctx.request {
            Some(req) => match req.ip() {
                Some(ip) => self.regex.is_match(ip),
                None => true, // NotApplicable = pass
            },
            None => true, // NotApplicable = pass
        }
    }
}

/// Compiled header filter - holds Vec<(lowercase key, compiled regex)> for header matching
/// Returns true if no request (NotApplicable = pass)
pub struct CompiledHeaderFilter {
    conditions: Vec<(String, Regex)>,
}

impl CompiledHeaderFilter {
    /// Compile a header filter from a pattern like "KEY1=val1,KEY2=val2"
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let mut conditions = Vec::new();

        for pair in pattern.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }

            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: format!("Invalid KEY=value format: {}", pair),
                });
            }

            // Store key as lowercase for case-insensitive matching
            let key = parts[0].trim().to_lowercase();
            let value_pattern = parts[1].trim();
            let regex = compile_anchored_regex(value_pattern)?;

            conditions.push((key, regex));
        }

        Ok(Self { conditions })
    }
}

impl CompiledDynamicFilter for CompiledHeaderFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        match &ctx.request {
            Some(req) => {
                // Build lowercase map for case-insensitive matching
                let headers_lower: std::collections::HashMap<String, &String> = req
                    .headers
                    .iter()
                    .map(|(k, v)| (k.to_lowercase(), v))
                    .collect();

                for (key, regex) in &self.conditions {
                    match headers_lower.get(key) {
                        Some(actual_value) => {
                            if !regex.is_match(actual_value) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            None => true, // NotApplicable = pass
        }
    }
}

/// Compiled context filter - holds Vec<(key, compiled regex)> for custom context matching
pub struct CompiledContextFilter {
    conditions: Vec<(String, Regex)>,
}

impl CompiledContextFilter {
    /// Compile a context filter from a pattern like "KEY1=val1,KEY2=val2"
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let mut conditions = Vec::new();

        for pair in pattern.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }

            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: format!("Invalid KEY=value format: {}", pair),
                });
            }

            let key = parts[0].trim().to_string();
            let value_pattern = parts[1].trim();
            let regex = compile_anchored_regex(value_pattern)?;

            conditions.push((key, regex));
        }

        Ok(Self { conditions })
    }
}

impl CompiledDynamicFilter for CompiledContextFilter {
    fn check(&self, ctx: &DynamicContext) -> bool {
        for (key, regex) in &self.conditions {
            match ctx.custom.get(key) {
                Some(actual_value) => {
                    if !regex.is_match(actual_value) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

/// Compiled probability filter - holds parsed probability value (0-100)
/// No regex compilation needed
pub struct CompiledProbabilityFilter {
    probability: f64,
}

impl CompiledProbabilityFilter {
    /// Compile a probability filter from a pattern like "25" (25% chance)
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let probability: f64 = pattern.parse().map_err(|_| SettingsError::InvalidRegex {
            pattern: pattern.to_string(),
            error: "Invalid probability value (expected number 0-100)".to_string(),
        })?;
        Ok(Self { probability })
    }
}

impl CompiledDynamicFilter for CompiledProbabilityFilter {
    fn check(&self, _ctx: &DynamicContext) -> bool {
        if self.probability <= 0.0 {
            return false;
        }
        if self.probability >= 100.0 {
            return true;
        }

        let mut rng = rand::thread_rng();
        let roll: f64 = rng.gen_range(0.0..100.0);

        roll < self.probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{CustomContext, Request};
    use std::collections::HashMap;

    fn make_ctx_with_request(path: &str, headers: HashMap<String, String>) -> DynamicContext {
        DynamicContext {
            request: Some(Request {
                method: "GET".to_string(),
                path: path.to_string(),
                headers,
            }),
            custom: CustomContext::new(),
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
        let ctx = DynamicContext::default();
        assert_eq!(filter.check("/api/.*", &ctx), FilterResult::NotApplicable);
    }

    #[test]
    fn test_host_filter_match() {
        let filter = HostFilter;
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), "api.example.com".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert_eq!(
            filter.check("api\\.example\\.com", &ctx),
            FilterResult::Match
        );
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
        let mut custom = CustomContext::new();
        custom.push_layer(
            [
                ("user_id".to_string(), "123".to_string()),
                ("role".to_string(), "admin".to_string()),
            ]
            .into(),
        );
        let ctx = DynamicContext {
            request: None,
            custom,
        };
        assert_eq!(
            filter.check("user_id=123,role=admin", &ctx),
            FilterResult::Match
        );
    }

    #[test]
    fn test_context_filter_regex_value() {
        let filter = ContextFilter;
        let mut custom = CustomContext::new();
        custom.push_layer([("user_id".to_string(), "12345".to_string())].into());
        let ctx = DynamicContext {
            request: None,
            custom,
        };
        assert_eq!(filter.check("user_id=123.*", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_probability_filter_zero() {
        let filter = ProbabilityFilter;
        let ctx = DynamicContext::default();
        // 0% should always NoMatch
        assert_eq!(filter.check("0", &ctx), FilterResult::NoMatch);
    }

    #[test]
    fn test_probability_filter_hundred() {
        let filter = ProbabilityFilter;
        let ctx = DynamicContext::default();
        // 100% should always Match
        assert_eq!(filter.check("100", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_probability_filter_invalid() {
        let filter = ProbabilityFilter;
        let ctx = DynamicContext::default();
        assert_eq!(filter.check("abc", &ctx), FilterResult::NoMatch);
    }
}

#[cfg(test)]
mod compiled_dynamic_tests {
    use super::*;
    use crate::context::{CustomContext, Request};
    use std::collections::HashMap;

    fn make_ctx_with_request(path: &str, headers: HashMap<String, String>) -> DynamicContext {
        DynamicContext {
            request: Some(Request {
                method: "GET".to_string(),
                path: path.to_string(),
                headers,
            }),
            custom: CustomContext::new(),
        }
    }

    // CompiledUrlPathFilter tests
    #[test]
    fn test_compiled_url_path_filter_match() {
        let filter = CompiledUrlPathFilter::compile("/api/.*").unwrap();
        let ctx = make_ctx_with_request("/api/users", HashMap::new());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_url_path_filter_no_match() {
        let filter = CompiledUrlPathFilter::compile("/api/.*").unwrap();
        let ctx = make_ctx_with_request("/admin/users", HashMap::new());
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_url_path_filter_no_request_returns_true() {
        // NotApplicable = pass (returns true)
        let filter = CompiledUrlPathFilter::compile("/api/.*").unwrap();
        let ctx = DynamicContext::default();
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_url_path_filter_case_insensitive() {
        let filter = CompiledUrlPathFilter::compile("/API/.*").unwrap();
        let ctx = make_ctx_with_request("/api/users", HashMap::new());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_url_path_filter_anchored() {
        let filter = CompiledUrlPathFilter::compile("api").unwrap();
        let ctx = make_ctx_with_request("/api/users", HashMap::new());
        // Pattern "api" should NOT match "/api/users" due to anchoring
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_url_path_filter_invalid_regex() {
        let result = CompiledUrlPathFilter::compile("(unclosed");
        assert!(result.is_err());
    }

    // CompiledHostFilter tests
    #[test]
    fn test_compiled_host_filter_match() {
        let filter = CompiledHostFilter::compile("api\\.example\\.com").unwrap();
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), "api.example.com".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_host_filter_no_match() {
        let filter = CompiledHostFilter::compile("api\\.example\\.com").unwrap();
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), "other.example.com".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_host_filter_no_request_returns_true() {
        let filter = CompiledHostFilter::compile("api\\.example\\.com").unwrap();
        let ctx = DynamicContext::default();
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_host_filter_no_host_returns_true() {
        let filter = CompiledHostFilter::compile("api\\.example\\.com").unwrap();
        let ctx = make_ctx_with_request("/", HashMap::new());
        assert!(filter.check(&ctx));
    }

    // CompiledEmailFilter tests
    #[test]
    fn test_compiled_email_filter_match() {
        let filter = CompiledEmailFilter::compile(".*@cian\\.ru").unwrap();
        let mut headers = HashMap::new();
        headers.insert("x-real-email".to_string(), "user@cian.ru".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_email_filter_no_match() {
        let filter = CompiledEmailFilter::compile(".*@cian\\.ru").unwrap();
        let mut headers = HashMap::new();
        headers.insert("x-real-email".to_string(), "user@other.com".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_email_filter_no_request_returns_true() {
        let filter = CompiledEmailFilter::compile(".*@cian\\.ru").unwrap();
        let ctx = DynamicContext::default();
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_email_filter_no_email_returns_true() {
        let filter = CompiledEmailFilter::compile(".*@cian\\.ru").unwrap();
        let ctx = make_ctx_with_request("/", HashMap::new());
        assert!(filter.check(&ctx));
    }

    // CompiledIpFilter tests
    #[test]
    fn test_compiled_ip_filter_match() {
        let filter = CompiledIpFilter::compile("192\\.168\\..*").unwrap();
        let mut headers = HashMap::new();
        headers.insert("x-real-ip".to_string(), "192.168.1.100".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_ip_filter_no_match() {
        let filter = CompiledIpFilter::compile("192\\.168\\..*").unwrap();
        let mut headers = HashMap::new();
        headers.insert("x-real-ip".to_string(), "10.0.0.1".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_ip_filter_no_request_returns_true() {
        let filter = CompiledIpFilter::compile("192\\.168\\..*").unwrap();
        let ctx = DynamicContext::default();
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_ip_filter_no_ip_returns_true() {
        let filter = CompiledIpFilter::compile("192\\.168\\..*").unwrap();
        let ctx = make_ctx_with_request("/", HashMap::new());
        assert!(filter.check(&ctx));
    }

    // CompiledHeaderFilter tests
    #[test]
    fn test_compiled_header_filter_match() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled").unwrap();
        let mut headers = HashMap::new();
        headers.insert("X-Feature".to_string(), "enabled".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_case_insensitive() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled").unwrap();
        let mut headers = HashMap::new();
        headers.insert("x-feature".to_string(), "enabled".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_no_match() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled").unwrap();
        let mut headers = HashMap::new();
        headers.insert("X-Feature".to_string(), "disabled".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_missing_header() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled").unwrap();
        let ctx = make_ctx_with_request("/", HashMap::new());
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_multiple_match() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled,X-Version=v2").unwrap();
        let mut headers = HashMap::new();
        headers.insert("X-Feature".to_string(), "enabled".to_string());
        headers.insert("X-Version".to_string(), "v2".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_multiple_partial_no_match() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled,X-Version=v2").unwrap();
        let mut headers = HashMap::new();
        headers.insert("X-Feature".to_string(), "enabled".to_string());
        // X-Version missing
        let ctx = make_ctx_with_request("/", headers);
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_no_request_returns_true() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled").unwrap();
        let ctx = DynamicContext::default();
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_regex_value() {
        let filter = CompiledHeaderFilter::compile("X-Version=v.*").unwrap();
        let mut headers = HashMap::new();
        headers.insert("X-Version".to_string(), "v2.1.0".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_header_filter_invalid_format() {
        let result = CompiledHeaderFilter::compile("invalid_no_equals");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_header_filter_invalid_regex() {
        let result = CompiledHeaderFilter::compile("X-Feature=(unclosed");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_header_filter_empty_pattern() {
        // Empty pattern should compile (no conditions means always true)
        let filter = CompiledHeaderFilter::compile("").unwrap();
        let ctx = make_ctx_with_request("/", HashMap::new());
        assert!(filter.check(&ctx));
    }

    // CompiledContextFilter tests
    #[test]
    fn test_compiled_context_filter_match() {
        let filter = CompiledContextFilter::compile("user_id=123,role=admin").unwrap();
        let mut custom = CustomContext::new();
        custom.push_layer(
            [
                ("user_id".to_string(), "123".to_string()),
                ("role".to_string(), "admin".to_string()),
            ]
            .into(),
        );
        let ctx = DynamicContext {
            request: None,
            custom,
        };
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_context_filter_no_match() {
        let filter = CompiledContextFilter::compile("user_id=123").unwrap();
        let mut custom = CustomContext::new();
        custom.push_layer([("user_id".to_string(), "456".to_string())].into());
        let ctx = DynamicContext {
            request: None,
            custom,
        };
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_context_filter_missing_key() {
        let filter = CompiledContextFilter::compile("user_id=123").unwrap();
        let ctx = DynamicContext::default();
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_context_filter_regex_value() {
        let filter = CompiledContextFilter::compile("user_id=123.*").unwrap();
        let mut custom = CustomContext::new();
        custom.push_layer([("user_id".to_string(), "12345".to_string())].into());
        let ctx = DynamicContext {
            request: None,
            custom,
        };
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_context_filter_invalid_format() {
        let result = CompiledContextFilter::compile("invalid_no_equals");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_context_filter_invalid_regex() {
        let result = CompiledContextFilter::compile("user_id=(unclosed");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_context_filter_empty_pattern() {
        // Empty pattern should compile (no conditions means always true)
        let filter = CompiledContextFilter::compile("").unwrap();
        let ctx = DynamicContext::default();
        assert!(filter.check(&ctx));
    }

    // CompiledProbabilityFilter tests
    #[test]
    fn test_compiled_probability_filter_zero() {
        let filter = CompiledProbabilityFilter::compile("0").unwrap();
        let ctx = DynamicContext::default();
        // 0% should always return false
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_probability_filter_hundred() {
        let filter = CompiledProbabilityFilter::compile("100").unwrap();
        let ctx = DynamicContext::default();
        // 100% should always return true
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_probability_filter_invalid() {
        let result = CompiledProbabilityFilter::compile("abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_probability_filter_negative() {
        let filter = CompiledProbabilityFilter::compile("-10").unwrap();
        let ctx = DynamicContext::default();
        // Negative should always return false
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_probability_filter_over_hundred() {
        let filter = CompiledProbabilityFilter::compile("150").unwrap();
        let ctx = DynamicContext::default();
        // Over 100% should always return true
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_probability_filter_float() {
        // Should compile with float value
        let filter = CompiledProbabilityFilter::compile("50.5").unwrap();
        // Just verify it compiles - randomness makes it hard to test
        let ctx = DynamicContext::default();
        // Run multiple times, one of them should pass (though randomness applies)
        let _ = filter.check(&ctx);
    }
}
