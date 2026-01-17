// lib/runtime-settings/src/filters/static_filters.rs
use super::{CompiledStaticFilter, FilterResult, StaticFilter};
use crate::context::StaticContext;
use crate::error::SettingsError;
use regex::{Regex, RegexBuilder};
use semver::Version;

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

/// Helper to parse "KEY1=value1,KEY2=value2" format and check against a map
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

/// environment: "KEY1=val1,KEY2=val2" against ctx.environment
pub struct EnvironmentFilter;

impl StaticFilter for EnvironmentFilter {
    fn name(&self) -> &'static str {
        "environment"
    }

    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult {
        check_map_filter(pattern, &ctx.environment)
    }
}

/// library_version: "pkg>=1.0.0,pkg<2.0.0" against ctx.libraries_versions
pub struct LibraryVersionFilter;

impl StaticFilter for LibraryVersionFilter {
    fn name(&self) -> &'static str {
        "library_version"
    }

    fn check(&self, pattern: &str, ctx: &StaticContext) -> FilterResult {
        // Parse pattern like "pkg>=1.0.0,pkg<2.0.0" or "pkg=1.2.3"
        for spec in pattern.split(',') {
            let spec = spec.trim();
            if spec.is_empty() {
                continue;
            }

            // Find the operator position
            let (pkg_name, op, version_str) = if let Some(pos) = spec.find(">=") {
                (&spec[..pos], ">=", &spec[pos + 2..])
            } else if let Some(pos) = spec.find("<=") {
                (&spec[..pos], "<=", &spec[pos + 2..])
            } else if let Some(pos) = spec.find('>') {
                (&spec[..pos], ">", &spec[pos + 1..])
            } else if let Some(pos) = spec.find('<') {
                (&spec[..pos], "<", &spec[pos + 1..])
            } else if let Some(pos) = spec.find('=') {
                (&spec[..pos], "=", &spec[pos + 1..])
            } else {
                return FilterResult::NoMatch;
            };

            let pkg_name = pkg_name.trim();
            let version_str = version_str.trim();

            // Get installed version
            let installed = match ctx.libraries_versions.get(pkg_name) {
                Some(v) => v,
                None => return FilterResult::NoMatch,
            };

            // Parse required version
            let required = match Version::parse(version_str) {
                Ok(v) => v,
                Err(_) => return FilterResult::NoMatch,
            };

            // Check condition
            let matches = match op {
                ">=" => installed >= &required,
                "<=" => installed <= &required,
                ">" => installed > &required,
                "<" => installed < &required,
                "=" => installed == &required,
                _ => false,
            };

            if !matches {
                return FilterResult::NoMatch;
            }
        }
        FilterResult::Match
    }
}

// =============================================================================
// Compiled Static Filters (pre-compiled regex for hot path optimization)
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

/// Compiled application filter - holds pre-compiled regex, checks against ctx.application
pub struct CompiledApplicationFilter {
    regex: Regex,
}

impl CompiledApplicationFilter {
    /// Compile an application filter from a pattern
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let regex = compile_anchored_regex(pattern)?;
        Ok(Self { regex })
    }
}

impl CompiledStaticFilter for CompiledApplicationFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        self.regex.is_match(&ctx.application)
    }
}

/// Compiled server filter - holds pre-compiled regex, checks against ctx.server
pub struct CompiledServerFilter {
    regex: Regex,
}

impl CompiledServerFilter {
    /// Compile a server filter from a pattern
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let regex = compile_anchored_regex(pattern)?;
        Ok(Self { regex })
    }
}

impl CompiledStaticFilter for CompiledServerFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        self.regex.is_match(&ctx.server)
    }
}

/// Compiled mcs_run_env filter - holds pre-compiled regex, checks against ctx.mcs_run_env
/// IMPORTANT: Returns false when mcs_run_env is None (matches Python behavior)
pub struct CompiledMcsRunEnvFilter {
    regex: Regex,
}

impl CompiledMcsRunEnvFilter {
    /// Compile a mcs_run_env filter from a pattern
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let regex = compile_anchored_regex(pattern)?;
        Ok(Self { regex })
    }
}

impl CompiledStaticFilter for CompiledMcsRunEnvFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        // Returns false when mcs_run_env is None (matches Python behavior)
        match &ctx.mcs_run_env {
            Some(env) => self.regex.is_match(env),
            None => false,
        }
    }
}

/// Compiled environment filter - holds Vec<(key, compiled regex)> for KEY=value conditions
pub struct CompiledEnvironmentFilter {
    conditions: Vec<(String, Regex)>,
}

impl CompiledEnvironmentFilter {
    /// Compile an environment filter from a pattern like "KEY1=val1,KEY2=val2"
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

impl CompiledStaticFilter for CompiledEnvironmentFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        for (key, regex) in &self.conditions {
            match ctx.environment.get(key) {
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

/// Version comparison operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOp {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl VersionOp {
    /// Compare two versions using this operator
    fn compare(&self, installed: &Version, required: &Version) -> bool {
        match self {
            VersionOp::Eq => installed == required,
            VersionOp::Gt => installed > required,
            VersionOp::Gte => installed >= required,
            VersionOp::Lt => installed < required,
            VersionOp::Lte => installed <= required,
        }
    }
}

/// Compiled library version filter - holds Vec<(package, op, version)> for version constraints
pub struct CompiledLibraryVersionFilter {
    constraints: Vec<(String, VersionOp, Version)>,
}

impl CompiledLibraryVersionFilter {
    /// Compile a library version filter from a pattern like "pkg>=1.0.0,pkg<2.0.0"
    pub fn compile(pattern: &str) -> Result<Self, SettingsError> {
        let mut constraints = Vec::new();

        for spec in pattern.split(',') {
            let spec = spec.trim();
            if spec.is_empty() {
                continue;
            }

            // Find the operator position and parse
            let (pkg_name, op, version_str) = if let Some(pos) = spec.find(">=") {
                (&spec[..pos], VersionOp::Gte, &spec[pos + 2..])
            } else if let Some(pos) = spec.find("<=") {
                (&spec[..pos], VersionOp::Lte, &spec[pos + 2..])
            } else if let Some(pos) = spec.find('>') {
                (&spec[..pos], VersionOp::Gt, &spec[pos + 1..])
            } else if let Some(pos) = spec.find('<') {
                (&spec[..pos], VersionOp::Lt, &spec[pos + 1..])
            } else if let Some(pos) = spec.find('=') {
                (&spec[..pos], VersionOp::Eq, &spec[pos + 1..])
            } else {
                return Err(SettingsError::InvalidVersionSpec {
                    spec: spec.to_string(),
                });
            };

            let pkg_name = pkg_name.trim().to_string();
            let version_str = version_str.trim();

            let version =
                Version::parse(version_str).map_err(|_| SettingsError::InvalidVersionSpec {
                    spec: spec.to_string(),
                })?;

            constraints.push((pkg_name, op, version));
        }

        Ok(Self { constraints })
    }
}

impl CompiledStaticFilter for CompiledLibraryVersionFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        for (pkg_name, op, required) in &self.constraints {
            match ctx.libraries_versions.get(pkg_name) {
                Some(installed) => {
                    if !op.compare(installed, required) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

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

    #[test]
    fn test_environment_filter_single_match() {
        let filter = EnvironmentFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "prod".to_string());
        assert_eq!(filter.check("ENV=prod", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_environment_filter_multiple_match() {
        let filter = EnvironmentFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "prod".to_string());
        ctx.environment
            .insert("DEBUG".to_string(), "false".to_string());
        assert_eq!(
            filter.check("ENV=prod,DEBUG=false", &ctx),
            FilterResult::Match
        );
    }

    #[test]
    fn test_environment_filter_partial_no_match() {
        let filter = EnvironmentFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "prod".to_string());
        // DEBUG is missing
        assert_eq!(
            filter.check("ENV=prod,DEBUG=false", &ctx),
            FilterResult::NoMatch
        );
    }

    #[test]
    fn test_environment_filter_regex_value() {
        let filter = EnvironmentFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "production".to_string());
        assert_eq!(filter.check("ENV=prod.*", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_library_version_filter_exact_match() {
        let filter = LibraryVersionFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(1, 2, 3));
        assert_eq!(filter.check("my-lib=1.2.3", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_library_version_filter_gte() {
        let filter = LibraryVersionFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(2, 0, 0));
        assert_eq!(filter.check("my-lib>=1.0.0", &ctx), FilterResult::Match);
    }

    #[test]
    fn test_library_version_filter_range() {
        let filter = LibraryVersionFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(1, 5, 0));
        assert_eq!(
            filter.check("my-lib>=1.0.0,my-lib<2.0.0", &ctx),
            FilterResult::Match
        );
    }

    #[test]
    fn test_library_version_filter_not_installed() {
        let filter = LibraryVersionFilter;
        let ctx = make_static_ctx("app", "server");
        assert_eq!(filter.check("my-lib>=1.0.0", &ctx), FilterResult::NoMatch);
    }

    #[test]
    fn test_library_version_filter_version_too_low() {
        let filter = LibraryVersionFilter;
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(0, 9, 0));
        assert_eq!(filter.check("my-lib>=1.0.0", &ctx), FilterResult::NoMatch);
    }
}

#[cfg(test)]
mod compiled_tests {
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

    // CompiledApplicationFilter tests
    #[test]
    fn test_compiled_application_filter_match() {
        let filter = CompiledApplicationFilter::compile("my-service").unwrap();
        let ctx = make_static_ctx("my-service", "server1");
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_application_filter_regex_match() {
        let filter = CompiledApplicationFilter::compile("my-service-.*").unwrap();
        let ctx = make_static_ctx("my-service-prod", "server1");
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_application_filter_no_match() {
        let filter = CompiledApplicationFilter::compile("my-service").unwrap();
        let ctx = make_static_ctx("other-service", "server1");
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_application_filter_case_insensitive() {
        let filter = CompiledApplicationFilter::compile("PROD").unwrap();
        let ctx = make_static_ctx("prod", "server1");
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_application_filter_anchored() {
        let filter = CompiledApplicationFilter::compile("service").unwrap();
        let ctx = make_static_ctx("my-service-prod", "server1");
        // Pattern "service" should NOT match "my-service-prod" due to anchoring
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_application_filter_invalid_regex() {
        let result = CompiledApplicationFilter::compile("(unclosed");
        assert!(result.is_err());
    }

    // CompiledServerFilter tests
    #[test]
    fn test_compiled_server_filter_match() {
        let filter = CompiledServerFilter::compile("prod-server-.*").unwrap();
        let ctx = make_static_ctx("app", "prod-server-1");
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_server_filter_no_match() {
        let filter = CompiledServerFilter::compile("prod-.*").unwrap();
        let ctx = make_static_ctx("app", "dev-server");
        assert!(!filter.check(&ctx));
    }

    // CompiledMcsRunEnvFilter tests
    #[test]
    fn test_compiled_mcs_run_env_filter_match() {
        let filter = CompiledMcsRunEnvFilter::compile("PROD").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.mcs_run_env = Some("PROD".to_string());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_mcs_run_env_filter_no_match() {
        let filter = CompiledMcsRunEnvFilter::compile("PROD").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.mcs_run_env = Some("DEV".to_string());
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_mcs_run_env_filter_none_returns_false() {
        // IMPORTANT: Returns false when mcs_run_env is None (matches Python behavior)
        let filter = CompiledMcsRunEnvFilter::compile("PROD").unwrap();
        let ctx = make_static_ctx("app", "server");
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_mcs_run_env_filter_case_insensitive() {
        let filter = CompiledMcsRunEnvFilter::compile("prod").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.mcs_run_env = Some("PROD".to_string());
        assert!(filter.check(&ctx));
    }

    // CompiledEnvironmentFilter tests
    #[test]
    fn test_compiled_environment_filter_single_match() {
        let filter = CompiledEnvironmentFilter::compile("ENV=prod").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "prod".to_string());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_environment_filter_multiple_match() {
        let filter = CompiledEnvironmentFilter::compile("ENV=prod,DEBUG=false").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "prod".to_string());
        ctx.environment
            .insert("DEBUG".to_string(), "false".to_string());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_environment_filter_partial_no_match() {
        let filter = CompiledEnvironmentFilter::compile("ENV=prod,DEBUG=false").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "prod".to_string());
        // DEBUG is missing
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_environment_filter_regex_value() {
        let filter = CompiledEnvironmentFilter::compile("ENV=prod.*").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.environment
            .insert("ENV".to_string(), "production".to_string());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_environment_filter_invalid_format() {
        let result = CompiledEnvironmentFilter::compile("invalid_no_equals");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_environment_filter_invalid_regex() {
        let result = CompiledEnvironmentFilter::compile("ENV=(unclosed");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_environment_filter_empty_pattern() {
        // Empty pattern should compile (no conditions means always true)
        let filter = CompiledEnvironmentFilter::compile("").unwrap();
        let ctx = make_static_ctx("app", "server");
        assert!(filter.check(&ctx));
    }

    // CompiledLibraryVersionFilter tests
    #[test]
    fn test_compiled_library_version_filter_exact_match() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib=1.2.3").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(1, 2, 3));
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_gte() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib>=1.0.0").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(2, 0, 0));
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_gt() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib>1.0.0").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(1, 0, 1));
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_lte() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib<=2.0.0").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(1, 5, 0));
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_lt() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib<2.0.0").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(1, 9, 9));
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_range() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib>=1.0.0,my-lib<2.0.0").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(1, 5, 0));
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_not_installed() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib>=1.0.0").unwrap();
        let ctx = make_static_ctx("app", "server");
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_version_too_low() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib>=1.0.0").unwrap();
        let mut ctx = make_static_ctx("app", "server");
        ctx.libraries_versions
            .insert("my-lib".to_string(), semver::Version::new(0, 9, 0));
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter_invalid_spec() {
        let result = CompiledLibraryVersionFilter::compile("invalid_no_operator");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_library_version_filter_invalid_version() {
        let result = CompiledLibraryVersionFilter::compile("my-lib>=not_a_version");
        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_library_version_filter_empty_pattern() {
        // Empty pattern should compile (no constraints means always true)
        let filter = CompiledLibraryVersionFilter::compile("").unwrap();
        let ctx = make_static_ctx("app", "server");
        assert!(filter.check(&ctx));
    }

    // VersionOp tests
    #[test]
    fn test_version_op_eq() {
        let v1 = semver::Version::new(1, 0, 0);
        let v2 = semver::Version::new(1, 0, 0);
        assert!(VersionOp::Eq.compare(&v1, &v2));
    }

    #[test]
    fn test_version_op_gt() {
        let v1 = semver::Version::new(2, 0, 0);
        let v2 = semver::Version::new(1, 0, 0);
        assert!(VersionOp::Gt.compare(&v1, &v2));
        assert!(!VersionOp::Gt.compare(&v2, &v1));
    }

    #[test]
    fn test_version_op_gte() {
        let v1 = semver::Version::new(1, 0, 0);
        let v2 = semver::Version::new(1, 0, 0);
        assert!(VersionOp::Gte.compare(&v1, &v2));
    }

    #[test]
    fn test_version_op_lt() {
        let v1 = semver::Version::new(1, 0, 0);
        let v2 = semver::Version::new(2, 0, 0);
        assert!(VersionOp::Lt.compare(&v1, &v2));
        assert!(!VersionOp::Lt.compare(&v2, &v1));
    }

    #[test]
    fn test_version_op_lte() {
        let v1 = semver::Version::new(1, 0, 0);
        let v2 = semver::Version::new(1, 0, 0);
        assert!(VersionOp::Lte.compare(&v1, &v2));
    }
}
