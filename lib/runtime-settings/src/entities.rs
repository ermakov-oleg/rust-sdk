// lib/runtime-settings/src/entities.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One setting from any source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub priority: i64,
    #[serde(default)]
    pub filter: HashMap<String, String>,
    pub value: serde_json::Value,
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
    pub settings: Vec<Setting>,
    #[serde(default)]
    pub deleted: Vec<SettingKey>,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setting_deserialize() {
        let json = r#"{
            "key": "MY_KEY",
            "priority": 100,
            "filter": {"application": "my-app"},
            "value": "test-value"
        }"#;
        let setting: Setting = serde_json::from_str(json).unwrap();
        assert_eq!(setting.key, "MY_KEY");
        assert_eq!(setting.priority, 100);
        assert_eq!(
            setting.filter.get("application"),
            Some(&"my-app".to_string())
        );
    }

    #[test]
    fn test_setting_deserialize_without_filter() {
        let json = r#"{"key": "KEY", "priority": 0, "value": 123}"#;
        let setting: Setting = serde_json::from_str(json).unwrap();
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
