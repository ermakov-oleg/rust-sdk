// lib/runtime-settings/src/entities.rs
use crate::context::{DynamicContext, StaticContext};
use crate::error::SettingsError;
use crate::filters::{
    compile_dynamic_filter, compile_static_filter, is_static_filter, CompiledDynamicFilter,
    CompiledStaticFilter,
};
use dashmap::DashMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// Raw setting as deserialized from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSetting {
    pub key: String,
    pub priority: i64,
    #[serde(default)]
    pub filter: HashMap<String, String>,
    pub value: serde_json::Value,
}

/// One setting with compiled filters for efficient matching
pub struct Setting {
    pub key: String,
    pub priority: i64,
    pub value: serde_json::Value,
    pub static_filters: Vec<Box<dyn CompiledStaticFilter>>,
    pub dynamic_filters: Vec<Box<dyn CompiledDynamicFilter>>,
    /// Cache of deserialized values by TypeId
    value_cache: DashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl std::fmt::Debug for Setting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Setting")
            .field("key", &self.key)
            .field("priority", &self.priority)
            .field("value", &self.value)
            .field("static_filters_count", &self.static_filters.len())
            .field("dynamic_filters_count", &self.dynamic_filters.len())
            .field("cached_types_count", &self.value_cache.len())
            .finish()
    }
}

impl Setting {
    /// Compile a RawSetting into a Setting with pre-compiled filters
    pub fn compile(raw: RawSetting) -> Result<Self, SettingsError> {
        let mut static_filters: Vec<Box<dyn CompiledStaticFilter>> = Vec::new();
        let mut dynamic_filters: Vec<Box<dyn CompiledDynamicFilter>> = Vec::new();

        for (name, pattern) in &raw.filter {
            if is_static_filter(name) {
                static_filters.push(compile_static_filter(name, pattern)?);
            } else {
                // Try to compile as dynamic filter, ignore unknown filters
                if let Ok(filter) = compile_dynamic_filter(name, pattern) {
                    dynamic_filters.push(filter);
                }
                // Unknown filters are silently ignored for backwards compatibility
            }
        }

        Ok(Setting {
            key: raw.key,
            priority: raw.priority,
            value: raw.value,
            static_filters,
            dynamic_filters,
            value_cache: DashMap::new(),
        })
    }

    /// Check all static filters against the given context
    pub fn check_static_filters(&self, ctx: &StaticContext) -> bool {
        self.static_filters.iter().all(|f| f.check(ctx))
    }

    /// Check all dynamic filters against the given context
    pub fn check_dynamic_filters(&self, ctx: &DynamicContext) -> bool {
        self.dynamic_filters.iter().all(|f| f.check(ctx))
    }

    /// Get the setting value with caching by TypeId.
    ///
    /// On first access, deserializes JSON and stores in cache.
    /// Subsequent calls with the same type return the cached value.
    ///
    /// Note: there is a possible race condition on concurrent cache misses from multiple threads —
    /// both will deserialize and one will overwrite the other. This is safe (values are identical),
    /// just a bit of extra work on first accesses.
    pub fn get_value<T>(&self) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();

        // Check cache
        if let Some(cached) = self.value_cache.get(&type_id) {
            let arc_any: Arc<dyn Any + Send + Sync> = Arc::clone(cached.value());
            return Arc::downcast::<T>(arc_any).ok();
        }

        // Cache miss — deserialize
        let value: T = serde_json::from_value(self.value.clone()).ok()?;
        let arc_value = Arc::new(value);

        // Store in cache
        self.value_cache.insert(
            type_id,
            Arc::clone(&arc_value) as Arc<dyn Any + Send + Sync>,
        );

        Some(arc_value)
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
    fn test_raw_setting_deserialize_without_filter() {
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

    #[test]
    fn test_setting_compile_no_filters() {
        let raw = RawSetting {
            key: "KEY".to_string(),
            priority: 100,
            filter: HashMap::new(),
            value: serde_json::json!("value"),
        };
        let setting = Setting::compile(raw).unwrap();
        assert_eq!(setting.key, "KEY");
        assert_eq!(setting.priority, 100);
        assert!(setting.static_filters.is_empty());
        assert!(setting.dynamic_filters.is_empty());
    }

    #[test]
    fn test_setting_compile_with_static_filter() {
        let raw = RawSetting {
            key: "KEY".to_string(),
            priority: 100,
            filter: [("application".to_string(), "my-app".to_string())].into(),
            value: serde_json::json!("value"),
        };
        let setting = Setting::compile(raw).unwrap();
        assert_eq!(setting.static_filters.len(), 1);
        assert!(setting.dynamic_filters.is_empty());
    }

    #[test]
    fn test_setting_compile_with_dynamic_filter() {
        let raw = RawSetting {
            key: "KEY".to_string(),
            priority: 100,
            filter: [("url-path".to_string(), "/api/.*".to_string())].into(),
            value: serde_json::json!("value"),
        };
        let setting = Setting::compile(raw).unwrap();
        assert!(setting.static_filters.is_empty());
        assert_eq!(setting.dynamic_filters.len(), 1);
    }

    #[test]
    fn test_setting_compile_with_mixed_filters() {
        let raw = RawSetting {
            key: "KEY".to_string(),
            priority: 100,
            filter: [
                ("application".to_string(), "my-app".to_string()),
                ("server".to_string(), "server-1".to_string()),
                ("url-path".to_string(), "/api/.*".to_string()),
                ("email".to_string(), ".*@example.com".to_string()),
            ]
            .into(),
            value: serde_json::json!("value"),
        };
        let setting = Setting::compile(raw).unwrap();
        assert_eq!(setting.static_filters.len(), 2);
        assert_eq!(setting.dynamic_filters.len(), 2);
    }

    #[test]
    fn test_setting_compile_ignores_unknown_filters() {
        let raw = RawSetting {
            key: "KEY".to_string(),
            priority: 100,
            filter: [("unknown_filter".to_string(), "value".to_string())].into(),
            value: serde_json::json!("value"),
        };
        let setting = Setting::compile(raw).unwrap();
        assert!(setting.static_filters.is_empty());
        assert!(setting.dynamic_filters.is_empty());
    }

    #[test]
    fn test_setting_check_static_filters() {
        let raw = RawSetting {
            key: "KEY".to_string(),
            priority: 100,
            filter: [("application".to_string(), "my-app".to_string())].into(),
            value: serde_json::json!("value"),
        };
        let setting = Setting::compile(raw).unwrap();

        let ctx_match = StaticContext {
            application: "my-app".to_string(),
            server: "server".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };
        assert!(setting.check_static_filters(&ctx_match));

        let ctx_no_match = StaticContext {
            application: "other-app".to_string(),
            server: "server".to_string(),
            environment: HashMap::new(),
            libraries_versions: HashMap::new(),
            mcs_run_env: None,
        };
        assert!(!setting.check_static_filters(&ctx_no_match));
    }

    #[test]
    fn test_setting_check_dynamic_filters() {
        let raw = RawSetting {
            key: "KEY".to_string(),
            priority: 100,
            filter: [("url-path".to_string(), "/api/.*".to_string())].into(),
            value: serde_json::json!("value"),
        };
        let setting = Setting::compile(raw).unwrap();

        let ctx_match = DynamicContext {
            request: Some(crate::context::Request {
                method: "GET".to_string(),
                path: "/api/users".to_string(),
                headers: HashMap::new(),
            }),
            custom: Default::default(),
        };
        assert!(setting.check_dynamic_filters(&ctx_match));

        let ctx_no_match = DynamicContext {
            request: Some(crate::context::Request {
                method: "GET".to_string(),
                path: "/other/path".to_string(),
                headers: HashMap::new(),
            }),
            custom: Default::default(),
        };
        assert!(!setting.check_dynamic_filters(&ctx_no_match));
    }
}
