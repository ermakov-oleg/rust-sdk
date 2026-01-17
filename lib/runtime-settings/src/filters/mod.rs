// lib/runtime-settings/src/filters/mod.rs
pub mod dynamic_filters;
pub mod static_filters;

use crate::context::{DynamicContext, StaticContext};
use crate::error::SettingsError;
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
    fn check(&self, pattern: &str, ctx: &DynamicContext) -> FilterResult;
}

/// Trait for pre-compiled static filters
pub trait CompiledStaticFilter: Send + Sync {
    fn check(&self, ctx: &StaticContext) -> bool;
}

/// Trait for pre-compiled dynamic filters
pub trait CompiledDynamicFilter: Send + Sync {
    fn check(&self, ctx: &DynamicContext) -> bool;
}

pub use dynamic_filters::*;
pub use static_filters::*;

/// Check all static filters against context. Returns true if all static filters match.
/// Non-static filters (dynamic filters, unknown filters) are skipped.
pub fn check_static_filters(filters: &HashMap<String, String>, ctx: &StaticContext) -> bool {
    for (name, pattern) in filters {
        // Only check known static filters, skip everything else
        if !is_static_filter(name) {
            continue;
        }

        // Compile and check the static filter
        match compile_static_filter(name, pattern) {
            Ok(compiled) => {
                if !compiled.check(ctx) {
                    return false;
                }
            }
            Err(_) => {
                // Invalid pattern - treat as no match
                return false;
            }
        }
    }
    true
}

// =============================================================================
// Filter Compilation Factory Functions
// =============================================================================

/// Known static filter names
const KNOWN_STATIC_FILTER_NAMES: &[&str] = &[
    "application",
    "server",
    "mcs_run_env",
    "environment",
    "library_version",
];

/// Check if a filter name is static
pub fn is_static_filter(name: &str) -> bool {
    KNOWN_STATIC_FILTER_NAMES.contains(&name)
}

/// Compile a static filter by name
pub fn compile_static_filter(
    name: &str,
    pattern: &str,
) -> Result<Box<dyn CompiledStaticFilter>, SettingsError> {
    match name {
        "application" => Ok(Box::new(CompiledApplicationFilter::compile(pattern)?)),
        "server" => Ok(Box::new(CompiledServerFilter::compile(pattern)?)),
        "mcs_run_env" => Ok(Box::new(CompiledMcsRunEnvFilter::compile(pattern)?)),
        "environment" => Ok(Box::new(CompiledEnvironmentFilter::compile(pattern)?)),
        "library_version" => Ok(Box::new(CompiledLibraryVersionFilter::compile(pattern)?)),
        _ => Err(SettingsError::InvalidRegex {
            pattern: pattern.to_string(),
            error: format!("Unknown static filter: {}", name),
        }),
    }
}

/// Compile a dynamic filter by name
pub fn compile_dynamic_filter(
    name: &str,
    pattern: &str,
) -> Result<Box<dyn CompiledDynamicFilter>, SettingsError> {
    match name {
        "url-path" => Ok(Box::new(CompiledUrlPathFilter::compile(pattern)?)),
        "host" => Ok(Box::new(CompiledHostFilter::compile(pattern)?)),
        "email" => Ok(Box::new(CompiledEmailFilter::compile(pattern)?)),
        "ip" => Ok(Box::new(CompiledIpFilter::compile(pattern)?)),
        "header" => Ok(Box::new(CompiledHeaderFilter::compile(pattern)?)),
        "context" => Ok(Box::new(CompiledContextFilter::compile(pattern)?)),
        "probability" => Ok(Box::new(CompiledProbabilityFilter::compile(pattern)?)),
        _ => Err(SettingsError::InvalidRegex {
            pattern: pattern.to_string(),
            error: format!("Unknown dynamic filter: {}", name),
        }),
    }
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
    fn test_check_static_filters_skips_dynamic() {
        // Dynamic filters should be skipped by check_static_filters
        let filters: HashMap<String, String> =
            [("url-path".to_string(), "/api/.*".to_string())].into();

        let ctx = StaticContext {
            application: "app".to_string(),
            server: "server".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };

        // Dynamic filters are skipped, so this should pass
        assert!(check_static_filters(&filters, &ctx));
    }

    #[test]
    fn test_check_static_filters_ignores_unknown() {
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
