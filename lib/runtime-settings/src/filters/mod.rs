// lib/runtime-settings/src/filters/mod.rs
pub mod dynamic_filters;
pub mod static_filters;

use crate::context::{Context, StaticContext};
use lazy_static::lazy_static;
use std::collections::HashMap;

/// Result of filter check
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterResult {
    Match,
    NoMatch,
    NotApplicable,
}

/// Static filter - checked once when loading settings
pub trait StaticFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult;
}

/// Dynamic filter - checked on every get()
pub trait DynamicFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, pattern: &str, ctx: &Context) -> FilterResult;
}

pub use dynamic_filters::*;
pub use static_filters::*;

lazy_static! {
    static ref STATIC_FILTERS: Vec<Box<dyn StaticFilter>> = vec![
        Box::new(static_filters::ApplicationFilter),
        Box::new(static_filters::ServerFilter),
        Box::new(static_filters::EnvironmentFilter),
        Box::new(static_filters::McsRunEnvFilter),
        Box::new(static_filters::LibraryVersionFilter),
    ];
    static ref DYNAMIC_FILTERS: Vec<Box<dyn DynamicFilter>> = vec![
        Box::new(dynamic_filters::UrlPathFilter),
        Box::new(dynamic_filters::HostFilter),
        Box::new(dynamic_filters::EmailFilter),
        Box::new(dynamic_filters::IpFilter),
        Box::new(dynamic_filters::HeaderFilter),
        Box::new(dynamic_filters::ContextFilter),
        Box::new(dynamic_filters::ProbabilityFilter),
    ];
    static ref STATIC_FILTER_NAMES: Vec<&'static str> =
        STATIC_FILTERS.iter().map(|f| f.name()).collect();
    static ref DYNAMIC_FILTER_NAMES: Vec<&'static str> =
        DYNAMIC_FILTERS.iter().map(|f| f.name()).collect();
}

/// Check all static filters. Returns true if all match (or NotApplicable).
pub fn check_static_filters(filters: &HashMap<String, String>, ctx: &StaticContext) -> bool {
    for (name, pattern) in filters {
        // Skip dynamic filters
        if DYNAMIC_FILTER_NAMES.contains(&name.as_str()) {
            continue;
        }

        // Find matching static filter
        if let Some(filter) = STATIC_FILTERS.iter().find(|f| f.name() == name) {
            match filter.check(pattern, ctx) {
                FilterResult::Match | FilterResult::NotApplicable => continue,
                FilterResult::NoMatch => return false,
            }
        }
        // Unknown filter - ignore
    }
    true
}

/// Check all dynamic filters. Returns true if all match (or NotApplicable).
pub fn check_dynamic_filters(filters: &HashMap<String, String>, ctx: &Context) -> bool {
    for (name, pattern) in filters {
        // Skip static filters
        if STATIC_FILTER_NAMES.contains(&name.as_str()) {
            continue;
        }

        // Find matching dynamic filter
        if let Some(filter) = DYNAMIC_FILTERS.iter().find(|f| f.name() == name) {
            match filter.check(pattern, ctx) {
                FilterResult::Match | FilterResult::NotApplicable => continue,
                FilterResult::NoMatch => return false,
            }
        }
        // Unknown filter - ignore
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_check_static_filters_all_match() {
        let filters: HashMap<String, String> = [
            ("application".to_string(), "my-app".to_string()),
            ("server".to_string(), "server-1".to_string()),
        ]
        .into();

        let ctx = StaticContext {
            application: "my-app".to_string(),
            server: "server-1".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };

        assert!(check_static_filters(&filters, &ctx));
    }

    #[test]
    fn test_check_static_filters_one_no_match() {
        let filters: HashMap<String, String> = [
            ("application".to_string(), "my-app".to_string()),
            ("server".to_string(), "other-server".to_string()),
        ]
        .into();

        let ctx = StaticContext {
            application: "my-app".to_string(),
            server: "server-1".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };

        assert!(!check_static_filters(&filters, &ctx));
    }

    #[test]
    fn test_check_dynamic_filters_all_match() {
        let filters: HashMap<String, String> =
            [("url-path".to_string(), "/api/.*".to_string())].into();

        let mut ctx = Context::default();
        ctx.request = Some(crate::context::Request {
            method: "GET".to_string(),
            path: "/api/users".to_string(),
            headers: HashMap::new(),
        });

        assert!(check_dynamic_filters(&filters, &ctx));
    }

    #[test]
    fn test_check_filters_ignores_unknown() {
        let filters: HashMap<String, String> =
            [("unknown_filter".to_string(), "value".to_string())].into();

        let ctx = StaticContext {
            application: "app".to_string(),
            server: "server".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };

        // Unknown filters should be ignored (return true)
        assert!(check_static_filters(&filters, &ctx));
    }
}
