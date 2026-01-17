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
        Err(e) => {
            tracing::warn!(pattern = %pattern, error = %e, "Failed to compile regex pattern");
            FilterResult::NoMatch
        }
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

// Placeholder structs for other filters (to be implemented later)
#[allow(dead_code)]
pub struct EnvironmentFilter;
#[allow(dead_code)]
pub struct LibraryVersionFilter;

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

    #[test]
    fn test_case_insensitive_matching() {
        let filter = ApplicationFilter;
        let ctx = make_static_ctx("prod", "server1");
        // Pattern "PROD" should match value "prod" (case-insensitive)
        assert_eq!(filter.check("PROD", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_invalid_regex_returns_no_match() {
        let filter = ApplicationFilter;
        let ctx = make_static_ctx("my-service", "server1");
        // Invalid regex (unclosed group) should return NoMatch
        assert_eq!(filter.check("(unclosed", &ctx), FilterResult::NoMatch);
    }

    #[test]
    fn test_anchoring_prevents_partial_match() {
        let filter = ApplicationFilter;
        let ctx = make_static_ctx("my-service-prod", "server1");
        // Pattern "service" should NOT match "my-service-prod" due to anchoring
        assert_eq!(filter.check("service", &ctx), FilterResult::NoMatch);
    }
}
