use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// KV v2 secret data with version metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvData {
    pub data: HashMap<String, serde_json::Value>,
    pub metadata: KvVersion,
}

/// Version information for a secret
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvVersion {
    pub version: u64,
    pub created_time: DateTime<Utc>,
    #[serde(default)]
    pub deletion_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub destroyed: bool,
}

/// Full metadata for a secret including all versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvMetadata {
    pub created_time: DateTime<Utc>,
    #[serde(default)]
    pub custom_metadata: Option<HashMap<String, String>>,
    pub versions: Vec<KvVersion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kv_data_deserialize() {
        let json = r#"{
            "data": {"username": "admin", "password": "secret"},
            "metadata": {
                "version": 1,
                "created_time": "2024-01-01T00:00:00Z",
                "destroyed": false
            }
        }"#;
        let data: KvData = serde_json::from_str(json).unwrap();
        assert_eq!(data.metadata.version, 1);
        assert_eq!(data.data.get("username").unwrap(), "admin");
    }

    #[test]
    fn test_kv_version_optional_fields() {
        let json = r#"{
            "version": 2,
            "created_time": "2024-01-01T00:00:00Z"
        }"#;
        let version: KvVersion = serde_json::from_str(json).unwrap();
        assert_eq!(version.version, 2);
        assert!(version.deletion_time.is_none());
        assert!(!version.destroyed);
    }
}
