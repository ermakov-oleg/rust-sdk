# Hot Path Optimization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Optimize `get()` performance by pre-compiling regex filters and fixing correctness issues.

**Architecture:** Replace runtime regex compilation with pre-compiled filters stored in Setting struct. Switch from tokio::sync::RwLock to std::sync::RwLock for faster synchronous access.

**Tech Stack:** Rust, regex crate, std::sync::RwLock

---

## Task 1: Add Compiled Filter Traits

**Files:**
- Modify: `lib/runtime-settings/src/filters/mod.rs`

**Step 1: Add new traits for compiled filters**

```rust
/// Trait for pre-compiled static filters
pub trait CompiledStaticFilter: Send + Sync {
    fn check(&self, ctx: &StaticContext) -> bool;
}

/// Trait for pre-compiled dynamic filters
pub trait CompiledDynamicFilter: Send + Sync {
    fn check(&self, ctx: &Context) -> bool;
}
```

**Step 2: Run tests**

Run: `cargo test -p runtime-settings`
Expected: All existing tests pass (traits are additive)

**Step 3: Commit**

```bash
git add lib/runtime-settings/src/filters/mod.rs
git commit -m "Add CompiledStaticFilter and CompiledDynamicFilter traits"
```

---

## Task 2: Implement Compiled Static Filters

**Files:**
- Modify: `lib/runtime-settings/src/filters/static_filters.rs`

**Step 1: Add compiled versions of static filters**

```rust
use super::CompiledStaticFilter;
use regex::Regex;

/// Compiled application filter
pub struct CompiledApplicationFilter {
    regex: Regex,
}

impl CompiledApplicationFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let anchored = format!("^(?:{})$", pattern);
        let regex = regex::RegexBuilder::new(&anchored)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        Ok(Self { regex })
    }
}

impl CompiledStaticFilter for CompiledApplicationFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        self.regex.is_match(&ctx.application)
    }
}

/// Compiled server filter
pub struct CompiledServerFilter {
    regex: Regex,
}

impl CompiledServerFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let anchored = format!("^(?:{})$", pattern);
        let regex = regex::RegexBuilder::new(&anchored)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        Ok(Self { regex })
    }
}

impl CompiledStaticFilter for CompiledServerFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        self.regex.is_match(&ctx.server)
    }
}

/// Compiled mcs_run_env filter - returns false when mcs_run_env is None (matches Python behavior)
pub struct CompiledMcsRunEnvFilter {
    regex: Regex,
}

impl CompiledMcsRunEnvFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let anchored = format!("^(?:{})$", pattern);
        let regex = regex::RegexBuilder::new(&anchored)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        Ok(Self { regex })
    }
}

impl CompiledStaticFilter for CompiledMcsRunEnvFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        match &ctx.mcs_run_env {
            Some(env) => self.regex.is_match(env),
            None => false,  // FIX: Python returns False, not NotApplicable
        }
    }
}

/// Compiled environment filter
pub struct CompiledEnvironmentFilter {
    conditions: Vec<(String, Regex)>,
}

impl CompiledEnvironmentFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let mut conditions = Vec::new();
        for pair in pattern.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(crate::error::SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: "Invalid KEY=value format".to_string(),
                });
            }
            let key = parts[0].trim().to_string();
            let value_pattern = parts[1].trim();
            let anchored = format!("^(?:{})$", value_pattern);
            let regex = regex::RegexBuilder::new(&anchored)
                .case_insensitive(true)
                .build()
                .map_err(|e| crate::error::SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: e.to_string(),
                })?;
            conditions.push((key, regex));
        }
        Ok(Self { conditions })
    }
}

impl CompiledStaticFilter for CompiledEnvironmentFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        for (key, regex) in &self.conditions {
            match ctx.environment.get(key) {
                Some(value) => {
                    if !regex.is_match(value) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

/// Compiled library_version filter
pub struct CompiledLibraryVersionFilter {
    constraints: Vec<(String, VersionOp, semver::Version)>,
}

#[derive(Clone, Copy)]
pub enum VersionOp {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl CompiledLibraryVersionFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let mut constraints = Vec::new();
        for spec in pattern.split(',') {
            let spec = spec.trim();
            if spec.is_empty() {
                continue;
            }
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
                return Err(crate::error::SettingsError::InvalidVersionSpec {
                    spec: spec.to_string(),
                });
            };
            let version = semver::Version::parse(version_str.trim()).map_err(|_| {
                crate::error::SettingsError::InvalidVersionSpec {
                    spec: spec.to_string(),
                }
            })?;
            constraints.push((pkg_name.trim().to_string(), op, version));
        }
        Ok(Self { constraints })
    }
}

impl CompiledStaticFilter for CompiledLibraryVersionFilter {
    fn check(&self, ctx: &StaticContext) -> bool {
        for (pkg_name, op, required) in &self.constraints {
            match ctx.libraries_versions.get(pkg_name) {
                Some(installed) => {
                    let matches = match op {
                        VersionOp::Eq => installed == required,
                        VersionOp::Gt => installed > required,
                        VersionOp::Gte => installed >= required,
                        VersionOp::Lt => installed < required,
                        VersionOp::Lte => installed <= required,
                    };
                    if !matches {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}
```

**Step 2: Add tests for compiled static filters**

```rust
#[cfg(test)]
mod compiled_tests {
    use super::*;
    use std::collections::HashMap;

    fn make_static_ctx() -> StaticContext {
        StaticContext {
            application: "my-app".to_string(),
            server: "server-1".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        }
    }

    #[test]
    fn test_compiled_application_filter() {
        let filter = CompiledApplicationFilter::compile("my-app").unwrap();
        let ctx = make_static_ctx();
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_application_filter_regex() {
        let filter = CompiledApplicationFilter::compile("my-.*").unwrap();
        let ctx = make_static_ctx();
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_mcs_run_env_none_returns_false() {
        let filter = CompiledMcsRunEnvFilter::compile("PROD").unwrap();
        let ctx = make_static_ctx();  // mcs_run_env is None
        assert!(!filter.check(&ctx));  // Should return false, not true
    }

    #[test]
    fn test_compiled_mcs_run_env_matches() {
        let filter = CompiledMcsRunEnvFilter::compile("PROD").unwrap();
        let mut ctx = make_static_ctx();
        ctx.mcs_run_env = Some("PROD".to_string());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_environment_filter() {
        let filter = CompiledEnvironmentFilter::compile("ENV=prod,DEBUG=false").unwrap();
        let mut ctx = make_static_ctx();
        ctx.environment.insert("ENV".to_string(), "prod".to_string());
        ctx.environment.insert("DEBUG".to_string(), "false".to_string());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_library_version_filter() {
        let filter = CompiledLibraryVersionFilter::compile("my-lib>=1.0.0,my-lib<2.0.0").unwrap();
        let mut ctx = make_static_ctx();
        ctx.libraries_versions.insert("my-lib".to_string(), semver::Version::new(1, 5, 0));
        assert!(filter.check(&ctx));
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p runtime-settings compiled_tests`
Expected: All new tests pass

**Step 4: Commit**

```bash
git add lib/runtime-settings/src/filters/static_filters.rs
git commit -m "Add compiled static filter implementations"
```

---

## Task 3: Implement Compiled Dynamic Filters

**Files:**
- Modify: `lib/runtime-settings/src/filters/dynamic_filters.rs`

**Step 1: Add compiled versions of dynamic filters**

```rust
use super::CompiledDynamicFilter;
use crate::context::Context;
use regex::Regex;

/// Compiled url-path filter
pub struct CompiledUrlPathFilter {
    regex: Regex,
}

impl CompiledUrlPathFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let anchored = format!("^(?:{})$", pattern);
        let regex = regex::RegexBuilder::new(&anchored)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledUrlPathFilter {
    fn check(&self, ctx: &Context) -> bool {
        match &ctx.request {
            Some(req) => self.regex.is_match(&req.path),
            None => true,  // NotApplicable = pass
        }
    }
}

/// Compiled host filter
pub struct CompiledHostFilter {
    regex: Regex,
}

impl CompiledHostFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let anchored = format!("^(?:{})$", pattern);
        let regex = regex::RegexBuilder::new(&anchored)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledHostFilter {
    fn check(&self, ctx: &Context) -> bool {
        match &ctx.request {
            Some(req) => match req.host() {
                Some(host) => self.regex.is_match(host),
                None => true,
            },
            None => true,
        }
    }
}

/// Compiled email filter
pub struct CompiledEmailFilter {
    regex: Regex,
}

impl CompiledEmailFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let anchored = format!("^(?:{})$", pattern);
        let regex = regex::RegexBuilder::new(&anchored)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledEmailFilter {
    fn check(&self, ctx: &Context) -> bool {
        match &ctx.request {
            Some(req) => match req.email() {
                Some(email) => self.regex.is_match(email),
                None => true,
            },
            None => true,
        }
    }
}

/// Compiled ip filter
pub struct CompiledIpFilter {
    regex: Regex,
}

impl CompiledIpFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let anchored = format!("^(?:{})$", pattern);
        let regex = regex::RegexBuilder::new(&anchored)
            .case_insensitive(true)
            .build()
            .map_err(|e| crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        Ok(Self { regex })
    }
}

impl CompiledDynamicFilter for CompiledIpFilter {
    fn check(&self, ctx: &Context) -> bool {
        match &ctx.request {
            Some(req) => match req.ip() {
                Some(ip) => self.regex.is_match(ip),
                None => true,
            },
            None => true,
        }
    }
}

/// Compiled header filter
pub struct CompiledHeaderFilter {
    conditions: Vec<(String, Regex)>,  // lowercase header name -> pattern
}

impl CompiledHeaderFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let mut conditions = Vec::new();
        for pair in pattern.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(crate::error::SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: "Invalid KEY=value format".to_string(),
                });
            }
            let key = parts[0].trim().to_lowercase();
            let value_pattern = parts[1].trim();
            let anchored = format!("^(?:{})$", value_pattern);
            let regex = regex::RegexBuilder::new(&anchored)
                .case_insensitive(true)
                .build()
                .map_err(|e| crate::error::SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: e.to_string(),
                })?;
            conditions.push((key, regex));
        }
        Ok(Self { conditions })
    }
}

impl CompiledDynamicFilter for CompiledHeaderFilter {
    fn check(&self, ctx: &Context) -> bool {
        match &ctx.request {
            Some(req) => {
                for (key, regex) in &self.conditions {
                    let value = req.headers.iter()
                        .find(|(k, _)| k.to_lowercase() == *key)
                        .map(|(_, v)| v);
                    match value {
                        Some(v) => {
                            if !regex.is_match(v) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            None => true,
        }
    }
}

/// Compiled context filter
pub struct CompiledContextFilter {
    conditions: Vec<(String, Regex)>,
}

impl CompiledContextFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let mut conditions = Vec::new();
        for pair in pattern.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(crate::error::SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: "Invalid KEY=value format".to_string(),
                });
            }
            let key = parts[0].trim().to_string();
            let value_pattern = parts[1].trim();
            let anchored = format!("^(?:{})$", value_pattern);
            let regex = regex::RegexBuilder::new(&anchored)
                .case_insensitive(true)
                .build()
                .map_err(|e| crate::error::SettingsError::InvalidRegex {
                    pattern: pattern.to_string(),
                    error: e.to_string(),
                })?;
            conditions.push((key, regex));
        }
        Ok(Self { conditions })
    }
}

impl CompiledDynamicFilter for CompiledContextFilter {
    fn check(&self, ctx: &Context) -> bool {
        for (key, regex) in &self.conditions {
            match ctx.custom.get(key) {
                Some(value) => {
                    if !regex.is_match(value) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

/// Compiled probability filter
pub struct CompiledProbabilityFilter {
    probability: f64,
}

impl CompiledProbabilityFilter {
    pub fn compile(pattern: &str) -> Result<Self, crate::error::SettingsError> {
        let probability: f64 = pattern.parse().map_err(|_| {
            crate::error::SettingsError::InvalidRegex {
                pattern: pattern.to_string(),
                error: "Invalid probability value".to_string(),
            }
        })?;
        Ok(Self { probability })
    }
}

impl CompiledDynamicFilter for CompiledProbabilityFilter {
    fn check(&self, _ctx: &Context) -> bool {
        if self.probability <= 0.0 {
            return false;
        }
        if self.probability >= 100.0 {
            return true;
        }
        let mut rng = rand::thread_rng();
        let roll: f64 = rand::Rng::gen_range(&mut rng, 0.0..100.0);
        roll < self.probability
    }
}
```

**Step 2: Add tests for compiled dynamic filters**

```rust
#[cfg(test)]
mod compiled_dynamic_tests {
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
    fn test_compiled_url_path_filter() {
        let filter = CompiledUrlPathFilter::compile("/api/.*").unwrap();
        let ctx = make_ctx_with_request("/api/users", HashMap::new());
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_url_path_filter_no_request() {
        let filter = CompiledUrlPathFilter::compile("/api/.*").unwrap();
        let ctx = Context::default();
        assert!(filter.check(&ctx));  // NotApplicable = pass
    }

    #[test]
    fn test_compiled_header_filter() {
        let filter = CompiledHeaderFilter::compile("X-Feature=enabled").unwrap();
        let mut headers = HashMap::new();
        headers.insert("x-feature".to_string(), "enabled".to_string());
        let ctx = make_ctx_with_request("/", headers);
        assert!(filter.check(&ctx));
    }

    #[test]
    fn test_compiled_probability_zero() {
        let filter = CompiledProbabilityFilter::compile("0").unwrap();
        let ctx = Context::default();
        assert!(!filter.check(&ctx));
    }

    #[test]
    fn test_compiled_probability_hundred() {
        let filter = CompiledProbabilityFilter::compile("100").unwrap();
        let ctx = Context::default();
        assert!(filter.check(&ctx));
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p runtime-settings compiled_dynamic_tests`
Expected: All tests pass

**Step 4: Commit**

```bash
git add lib/runtime-settings/src/filters/dynamic_filters.rs
git commit -m "Add compiled dynamic filter implementations"
```

---

## Task 4: Add Filter Compilation Factory Functions

**Files:**
- Modify: `lib/runtime-settings/src/filters/mod.rs`

**Step 1: Add factory functions and exports**

```rust
// Add to mod.rs
pub use static_filters::{
    CompiledApplicationFilter, CompiledServerFilter, CompiledMcsRunEnvFilter,
    CompiledEnvironmentFilter, CompiledLibraryVersionFilter, VersionOp,
};
pub use dynamic_filters::{
    CompiledUrlPathFilter, CompiledHostFilter, CompiledEmailFilter, CompiledIpFilter,
    CompiledHeaderFilter, CompiledContextFilter, CompiledProbabilityFilter,
};

use crate::error::SettingsError;

/// Known static filter names
const STATIC_FILTER_NAMES: &[&str] = &["application", "server", "mcs_run_env", "environment", "library_version"];

/// Check if a filter name is static
pub fn is_static_filter(name: &str) -> bool {
    STATIC_FILTER_NAMES.contains(&name)
}

/// Compile a static filter by name
pub fn compile_static_filter(name: &str, pattern: &str) -> Result<Box<dyn CompiledStaticFilter>, SettingsError> {
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
pub fn compile_dynamic_filter(name: &str, pattern: &str) -> Result<Box<dyn CompiledDynamicFilter>, SettingsError> {
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
```

**Step 2: Run tests**

Run: `cargo test -p runtime-settings`
Expected: All tests pass

**Step 3: Commit**

```bash
git add lib/runtime-settings/src/filters/mod.rs
git commit -m "Add filter compilation factory functions"
```

---

## Task 5: Update Setting Entity

**Files:**
- Modify: `lib/runtime-settings/src/entities.rs`

**Step 1: Add compiled filters to Setting**

```rust
use crate::filters::{CompiledDynamicFilter, CompiledStaticFilter};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw setting from JSON (for deserialization)
#[derive(Debug, Clone, Deserialize)]
pub struct RawSetting {
    pub key: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub filter: HashMap<String, String>,
    pub value: serde_json::Value,
}

/// Setting with compiled filters (used at runtime)
pub struct Setting {
    pub key: String,
    pub priority: i64,
    pub value: serde_json::Value,
    pub static_filters: Vec<Box<dyn CompiledStaticFilter>>,
    pub dynamic_filters: Vec<Box<dyn CompiledDynamicFilter>>,
}

impl Setting {
    /// Compile a RawSetting into a Setting with pre-compiled filters
    pub fn compile(raw: RawSetting) -> Result<Self, crate::error::SettingsError> {
        let mut static_filters: Vec<Box<dyn CompiledStaticFilter>> = Vec::new();
        let mut dynamic_filters: Vec<Box<dyn CompiledDynamicFilter>> = Vec::new();

        for (name, pattern) in &raw.filter {
            if crate::filters::is_static_filter(name) {
                static_filters.push(crate::filters::compile_static_filter(name, pattern)?);
            } else {
                dynamic_filters.push(crate::filters::compile_dynamic_filter(name, pattern)?);
            }
        }

        Ok(Self {
            key: raw.key,
            priority: raw.priority,
            value: raw.value,
            static_filters,
            dynamic_filters,
        })
    }

    /// Check all static filters
    pub fn check_static_filters(&self, ctx: &crate::context::StaticContext) -> bool {
        self.static_filters.iter().all(|f| f.check(ctx))
    }

    /// Check all dynamic filters
    pub fn check_dynamic_filters(&self, ctx: &crate::context::Context) -> bool {
        self.dynamic_filters.iter().all(|f| f.check(ctx))
    }
}

/// Identifier for deleting a setting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingKey {
    pub key: String,
    pub priority: i64,
}

/// Response from MCS
#[derive(Debug, Clone, Deserialize)]
pub struct McsResponse {
    pub settings: Vec<RawSetting>,
    #[serde(default)]
    pub deleted: Vec<SettingKey>,
    pub version: String,
}
```

**Step 2: Update tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_setting_deserialize() {
        let json = r#"{
            "key": "MY_KEY",
            "priority": 100,
            "filter": {"application": "my-app"},
            "value": "test-value"
        }"#;
        let setting: RawSetting = serde_json::from_str(json).unwrap();
        assert_eq!(setting.key, "MY_KEY");
        assert_eq!(setting.priority, 100);
        assert_eq!(
            setting.filter.get("application"),
            Some(&"my-app".to_string())
        );
    }

    #[test]
    fn test_setting_compile() {
        let raw = RawSetting {
            key: "MY_KEY".to_string(),
            priority: 100,
            filter: HashMap::from([
                ("application".to_string(), "my-app".to_string()),
                ("url-path".to_string(), "/api/.*".to_string()),
            ]),
            value: serde_json::json!("test"),
        };
        let setting = Setting::compile(raw).unwrap();
        assert_eq!(setting.key, "MY_KEY");
        assert_eq!(setting.static_filters.len(), 1);
        assert_eq!(setting.dynamic_filters.len(), 1);
    }

    #[test]
    fn test_setting_deserialize_without_filter() {
        let json = r#"{"key": "KEY", "priority": 0, "value": 123}"#;
        let setting: RawSetting = serde_json::from_str(json).unwrap();
        assert!(setting.filter.is_empty());
        assert_eq!(setting.value, serde_json::json!(123));
    }

    #[test]
    fn test_setting_key_deserialize() {
        let json = r#"{"key": "KEY", "priority": -1000000000000000000}"#;
        let key: SettingKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.key, "KEY");
        assert_eq!(key.priority, -1_000_000_000_000_000_000);
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p runtime-settings entities`
Expected: All tests pass

**Step 4: Commit**

```bash
git add lib/runtime-settings/src/entities.rs
git commit -m "Update Setting to use compiled filters"
```

---

## Task 6: Update Providers to Return RawSetting

**Files:**
- Modify: `lib/runtime-settings/src/providers/mod.rs`
- Modify: `lib/runtime-settings/src/providers/env.rs`
- Modify: `lib/runtime-settings/src/providers/file.rs`
- Modify: `lib/runtime-settings/src/providers/mcs.rs`

**Step 1: Update ProviderResponse to use RawSetting**

```rust
// providers/mod.rs
use crate::entities::{RawSetting, SettingKey};

pub struct ProviderResponse {
    pub settings: Vec<RawSetting>,
    pub deleted: Vec<SettingKey>,
    pub version: String,
}
```

**Step 2: Update EnvProvider**

```rust
// providers/env.rs - update to return RawSetting
use crate::entities::RawSetting;

// In load() method, change Setting to RawSetting
RawSetting {
    key: key.clone(),
    priority: self.default_priority(),
    filter: HashMap::new(),
    value,
}
```

**Step 3: Update FileProvider**

```rust
// providers/file.rs - FileSetting already deserializes to similar structure
// Convert FileSetting to RawSetting in load()
```

**Step 4: McsProvider already returns RawSetting via McsResponse**

**Step 5: Run tests**

Run: `cargo test -p runtime-settings`
Expected: All tests pass

**Step 6: Commit**

```bash
git add lib/runtime-settings/src/providers/
git commit -m "Update providers to use RawSetting"
```

---

## Task 7: Update RuntimeSettings to Use std::sync::RwLock and Compiled Filters

**Files:**
- Modify: `lib/runtime-settings/src/settings.rs`

**Step 1: Replace tokio::sync::RwLock with std::sync::RwLock**

```rust
use std::sync::RwLock;  // Changed from tokio::sync::RwLock
```

**Step 2: Update SettingsState to store compiled Settings**

```rust
struct SettingsState {
    version: String,
    settings: HashMap<String, Vec<Setting>>,  // Now uses compiled Setting
}
```

**Step 3: Update merge_settings to compile filters**

```rust
fn merge_settings(&self, response: ProviderResponse) {
    let mut state = self.state.write().unwrap();

    // Process deleted settings first
    for deleted in &response.deleted {
        if let Some(settings) = state.settings.get_mut(&deleted.key) {
            settings.retain(|s| s.priority != deleted.priority);
        }
    }

    // Process new/updated settings
    for raw_setting in response.settings {
        // Compile the setting
        let setting = match Setting::compile(raw_setting) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to compile setting filters, skipping");
                continue;
            }
        };

        // Check static filters
        if !setting.check_static_filters(&self.static_context) {
            if let Some(settings) = state.settings.get_mut(&setting.key) {
                settings.retain(|s| s.priority != setting.priority);
            }
            continue;
        }

        // Add or update setting
        let settings = state.settings.entry(setting.key.clone()).or_default();
        settings.retain(|s| s.priority != setting.priority);
        let pos = settings
            .iter()
            .position(|s| s.priority < setting.priority)
            .unwrap_or(settings.len());
        settings.insert(pos, setting);
    }

    if !response.version.is_empty() {
        state.version = response.version;
    }
}
```

**Step 4: Update get_internal to use compiled filters**

```rust
fn get_internal<T: DeserializeOwned>(&self, key: &str, ctx: &Context) -> Option<T> {
    let state = self.state.read().unwrap();  // std::sync::RwLock - no block_on needed

    let settings = state.settings.get(key)?;

    for setting in settings {
        if setting.check_dynamic_filters(ctx) {
            match serde_json::from_value(setting.value.clone()) {
                Ok(v) => return Some(v),
                Err(e) => {
                    tracing::warn!(key = key, error = %e, "Failed to deserialize setting value");
                    return None;
                }
            }
        }
    }

    None
}
```

**Step 5: Remove async from merge_settings (now uses sync RwLock)**

Change signature from `async fn merge_settings` to `fn merge_settings` and update all callers.

**Step 6: Run tests**

Run: `cargo test -p runtime-settings`
Expected: All tests pass

**Step 7: Commit**

```bash
git add lib/runtime-settings/src/settings.rs
git commit -m "Use std::sync::RwLock and compiled filters in RuntimeSettings"
```

---

## Task 8: Update lib.rs Exports and Remove Old Filter Code

**Files:**
- Modify: `lib/runtime-settings/src/lib.rs`
- Modify: `lib/runtime-settings/src/filters/mod.rs`

**Step 1: Update exports in lib.rs**

Remove exports for old filter functions, add exports for new compiled types if needed.

**Step 2: Remove old lazy_static filter registries**

Remove STATIC_FILTERS and DYNAMIC_FILTERS lazy_static registries and old check_* functions.

**Step 3: Clean up unused imports**

**Step 4: Run tests**

Run: `cargo test -p runtime-settings`
Expected: All tests pass

**Step 5: Run clippy**

Run: `cargo clippy -p runtime-settings`
Expected: No warnings

**Step 6: Commit**

```bash
git add lib/runtime-settings/src/
git commit -m "Clean up old filter code and update exports"
```

---

## Task 9: Run Full Test Suite and Verify

**Step 1: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy on entire workspace**

Run: `cargo clippy --all-targets`
Expected: No warnings

**Step 3: Run example**

Run: `cargo run -p example -- serve --address 0.0.0.0 --port 8080`
Expected: Compiles and runs

**Step 4: Commit any fixes**

```bash
git add -A
git commit -m "Fix any remaining issues from test run"
```

---

## Summary

| Task | Description |
|------|-------------|
| 1 | Add CompiledStaticFilter and CompiledDynamicFilter traits |
| 2 | Implement compiled static filters |
| 3 | Implement compiled dynamic filters |
| 4 | Add filter compilation factory functions |
| 5 | Update Setting entity to use compiled filters |
| 6 | Update providers to return RawSetting |
| 7 | Update RuntimeSettings to use std::sync::RwLock |
| 8 | Clean up old filter code |
| 9 | Run full test suite |
