// lib/runtime-settings/src/providers/env.rs

use super::{ProviderResponse, SettingsProvider};
use crate::entities::Setting;
use crate::error::SettingsError;
use async_trait::async_trait;
use std::collections::HashMap;

const ENV_PRIORITY: i64 = -1_000_000_000_000_000_000;

pub struct EnvProvider {
    environ: HashMap<String, String>,
}

impl EnvProvider {
    /// Create with custom environment (for testing)
    pub fn new(environ: HashMap<String, String>) -> Self {
        Self { environ }
    }

    /// Create with actual OS environment
    pub fn from_env() -> Self {
        Self {
            environ: std::env::vars().collect(),
        }
    }
}

#[async_trait]
impl SettingsProvider for EnvProvider {
    async fn load(&self, _current_version: &str) -> Result<ProviderResponse, SettingsError> {
        let settings: Vec<Setting> = self
            .environ
            .iter()
            .map(|(key, value)| {
                // Try to parse as JSON, fall back to string
                let json_value = serde_json::from_str(value)
                    .unwrap_or_else(|_| serde_json::Value::String(value.clone()));

                Setting {
                    key: key.clone(),
                    priority: ENV_PRIORITY,
                    filter: HashMap::new(),
                    value: json_value,
                }
            })
            .collect();

        Ok(ProviderResponse {
            settings,
            deleted: vec![],
            version: String::new(),
        })
    }

    fn default_priority(&self) -> i64 {
        ENV_PRIORITY
    }

    fn name(&self) -> &'static str {
        "env"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_env_provider_loads_env_vars() {
        let mut env = std::collections::HashMap::new();
        env.insert("MY_VAR".to_string(), "my_value".to_string());
        env.insert("MY_NUM".to_string(), "123".to_string());

        let provider = EnvProvider::new(env);
        let response = provider.load("").await.unwrap();

        assert!(response.settings.iter().any(|s| s.key == "MY_VAR"));
        assert!(response.settings.iter().any(|s| s.key == "MY_NUM"));
    }

    #[tokio::test]
    async fn test_env_provider_parses_json() {
        let mut env = std::collections::HashMap::new();
        env.insert("JSON_VAR".to_string(), r#"{"key": "value"}"#.to_string());

        let provider = EnvProvider::new(env);
        let response = provider.load("").await.unwrap();

        let setting = response
            .settings
            .iter()
            .find(|s| s.key == "JSON_VAR")
            .unwrap();
        assert_eq!(setting.value, serde_json::json!({"key": "value"}));
    }

    #[tokio::test]
    async fn test_env_provider_string_fallback() {
        let mut env = std::collections::HashMap::new();
        env.insert("STR_VAR".to_string(), "not json".to_string());

        let provider = EnvProvider::new(env);
        let response = provider.load("").await.unwrap();

        let setting = response
            .settings
            .iter()
            .find(|s| s.key == "STR_VAR")
            .unwrap();
        assert_eq!(setting.value, serde_json::json!("not json"));
    }

    #[tokio::test]
    async fn test_env_provider_priority() {
        let provider = EnvProvider::new(std::collections::HashMap::new());
        assert_eq!(provider.default_priority(), -1_000_000_000_000_000_000);
    }
}
