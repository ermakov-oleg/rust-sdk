// lib/runtime-settings/src/secrets/mod.rs

pub mod resolver;

use crate::error::SettingsError;

/// Key for navigating JSON structure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathKey {
    Field(String),
    Index(usize),
}

/// Information about a single secret usage in a setting value
#[derive(Debug, Clone)]
pub struct SecretUsage {
    /// Full Vault path including mount and `/data/`: "secret/data/db/creds"
    pub path: String,
    /// Key within the secret: "password"
    pub key: String,
    /// Where to substitute in JSON: ["connection", "password"]
    pub value_path: Vec<JsonPathKey>,
}
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use vault_client::VaultClient;

pub use resolver::{resolve_secrets, resolve_secrets_sync};

/// Parse secret usages from a JSON value during Setting compilation
pub fn find_secret_usages(value: &serde_json::Value) -> Result<Vec<SecretUsage>, SettingsError> {
    let mut usages = Vec::new();
    find_secrets_recursive(value, &mut Vec::new(), &mut usages)?;
    Ok(usages)
}

fn find_secrets_recursive(
    value: &serde_json::Value,
    current_path: &mut Vec<JsonPathKey>,
    usages: &mut Vec<SecretUsage>,
) -> Result<(), SettingsError> {
    match value {
        serde_json::Value::Object(map) => {
            // Check for {"$secret": "path:key"}
            if map.len() == 1 {
                if let Some(serde_json::Value::String(reference)) = map.get("$secret") {
                    let (path, key) = parse_secret_ref(reference)?;
                    usages.push(SecretUsage {
                        path,
                        key,
                        value_path: current_path.clone(),
                    });
                    return Ok(());
                }
            }

            // Recursively process fields
            for (field, v) in map {
                current_path.push(JsonPathKey::Field(field.clone()));
                find_secrets_recursive(v, current_path, usages)?;
                current_path.pop();
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                current_path.push(JsonPathKey::Index(i));
                find_secrets_recursive(v, current_path, usages)?;
                current_path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_secret_ref(reference: &str) -> Result<(String, String), SettingsError> {
    reference
        .split_once(':')
        .map(|(p, k)| (p.to_string(), k.to_string()))
        .ok_or_else(|| SettingsError::InvalidSecretReference {
            reference: reference.to_string(),
        })
}

/// Cached secret with metadata
struct CachedSecret {
    value: serde_json::Value,
    #[allow(dead_code)]
    lease_id: Option<String>,
    lease_duration: Option<Duration>,
    renewable: bool,
    fetched_at: Instant,
}

impl CachedSecret {
    fn needs_refresh(&self, threshold: f64) -> bool {
        match self.lease_duration {
            Some(duration) if self.renewable => {
                let elapsed = self.fetched_at.elapsed();
                let threshold_duration =
                    Duration::from_secs_f64(duration.as_secs_f64() * threshold);
                elapsed >= threshold_duration
            }
            _ => false,
        }
    }
}

pub struct SecretsService {
    client: Option<VaultClient>,
    cache: RwLock<HashMap<String, CachedSecret>>,
    refresh_intervals: HashMap<String, Duration>,
    version: AtomicU64,
}

impl SecretsService {
    /// Create without Vault (secrets will fail)
    pub fn new_without_vault() -> Self {
        Self {
            client: None,
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::load_refresh_intervals(),
            version: AtomicU64::new(0),
        }
    }

    /// Create with Vault client
    pub fn new(client: VaultClient) -> Self {
        Self {
            client: Some(client),
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::load_refresh_intervals(),
            version: AtomicU64::new(0),
        }
    }

    /// Get current version (for cache invalidation in Settings)
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    fn default_refresh_intervals() -> HashMap<String, Duration> {
        let mut intervals = HashMap::new();
        intervals.insert("kafka-certificates".to_string(), Duration::from_secs(600));
        intervals.insert("interservice-auth".to_string(), Duration::from_secs(60));
        intervals
    }

    fn load_refresh_intervals() -> HashMap<String, Duration> {
        let mut intervals = Self::default_refresh_intervals();

        if let Ok(json) = std::env::var("STATIC_SECRETS_REFRESH_INTERVALS") {
            match serde_json::from_str::<HashMap<String, u64>>(&json) {
                Ok(custom) => {
                    for (key, secs) in custom {
                        intervals.insert(key, Duration::from_secs(secs));
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Invalid STATIC_SECRETS_REFRESH_INTERVALS format");
                }
            }
        }

        intervals
    }

    /// Get secret value by path and key
    pub async fn get(&self, path: &str, key: &str) -> Result<serde_json::Value, SettingsError> {
        let client = self
            .client
            .as_ref()
            .ok_or(SettingsError::SecretWithoutVault)?;

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(path) {
                if let Some(value) = cached.value.get(key) {
                    return Ok(value.clone());
                }
            }
        }

        // Fetch from Vault using vault-client
        let kv_data = client
            .kv_read_raw(path)
            .await
            .map_err(|e| SettingsError::Vault(e.to_string()))?;

        // Convert HashMap to Value for caching
        let secret: serde_json::Value = serde_json::to_value(&kv_data.data)
            .map_err(|e| SettingsError::Vault(e.to_string()))?;

        // Cache it
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                path.to_string(),
                CachedSecret {
                    value: secret.clone(),
                    lease_id: None,
                    lease_duration: None,
                    renewable: false,
                    fetched_at: Instant::now(),
                },
            );
        }

        secret
            .get(key)
            .cloned()
            .ok_or_else(|| SettingsError::SecretKeyNotFound {
                path: path.to_string(),
                key: key.to_string(),
            })
    }

    /// Synchronous get for use in RuntimeSettings::get()
    ///
    /// Uses block_in_place to fetch from Vault if not cached.
    /// Only works in multi-threaded tokio runtime.
    pub fn get_sync(&self, path: &str, key: &str) -> Result<serde_json::Value, SettingsError> {
        // Fast path: check cache with blocking read
        {
            let cache = self.cache.blocking_read();
            if let Some(cached) = cache.get(path) {
                if let Some(value) = cached.value.get(key) {
                    return Ok(value.clone());
                }
                // Path exists but key not found
                return Err(SettingsError::SecretKeyNotFound {
                    path: path.to_string(),
                    key: key.to_string(),
                });
            }
        }

        // Slow path: fetch from Vault using block_in_place
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.get(path, key))
        })
    }

    fn needs_static_refresh(&self, path: &str, cached: &CachedSecret) -> bool {
        if cached.renewable {
            return false;
        }
        for (pattern, interval) in &self.refresh_intervals {
            if path.contains(pattern) {
                return cached.fetched_at.elapsed() >= *interval;
            }
        }
        false
    }

    /// Refresh all cached secrets
    pub async fn refresh(&self) -> Result<(), SettingsError> {
        let client = match &self.client {
            Some(c) => c,
            None => return Ok(()),
        };

        let paths_to_refresh: Vec<String> = {
            let cache = self.cache.read().await;
            cache
                .iter()
                .filter(|(path, cached)| {
                    cached.needs_refresh(0.75) || self.needs_static_refresh(path, cached)
                })
                .map(|(path, _)| path.clone())
                .collect()
        };

        let mut any_changed = false;

        for path in paths_to_refresh {
            match client.kv_read_raw(&path).await {
                Ok(kv_data) => {
                    let new_value: serde_json::Value = match serde_json::to_value(&kv_data.data) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(path = %path, error = %e, "Failed to convert secret data");
                            continue;
                        }
                    };
                    let changed = self.update_cached_secret(&path, new_value).await;
                    if changed {
                        any_changed = true;
                        tracing::debug!(path = %path, "Secret value changed");
                    } else {
                        tracing::debug!(path = %path, "Secret refreshed (unchanged)");
                    }
                }
                Err(e) => {
                    tracing::warn!(path = %path, error = %e, "Failed to refresh secret");
                }
            }
        }

        if any_changed {
            self.version.fetch_add(1, Ordering::Release);
        }

        Ok(())
    }

    /// Update cached secret, returns true if value changed
    async fn update_cached_secret(&self, path: &str, new_value: serde_json::Value) -> bool {
        let mut cache = self.cache.write().await;

        let changed = cache
            .get(path)
            .map(|cached| cached.value != new_value)
            .unwrap_or(true);

        cache.insert(
            path.to_string(),
            CachedSecret {
                value: new_value,
                lease_id: None,
                lease_duration: None,
                renewable: false,
                fetched_at: Instant::now(),
            },
        );

        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_secret_usages_no_secrets() {
        let value = serde_json::json!({"host": "localhost", "port": 5432});
        let usages = find_secret_usages(&value).unwrap();
        assert!(usages.is_empty());
    }

    #[test]
    fn test_find_secret_usages_single_secret() {
        let value = serde_json::json!({
            "host": "localhost",
            "password": {"$secret": "secret/data/db/creds:password"}
        });
        let usages = find_secret_usages(&value).unwrap();
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].path, "secret/data/db/creds");
        assert_eq!(usages[0].key, "password");
        assert_eq!(usages[0].value_path, vec![JsonPathKey::Field("password".to_string())]);
    }

    #[test]
    fn test_find_secret_usages_nested_secret() {
        let value = serde_json::json!({
            "database": {
                "connection": {
                    "password": {"$secret": "secret/data/db/creds:password"}
                }
            }
        });
        let usages = find_secret_usages(&value).unwrap();
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].path, "secret/data/db/creds");
        assert_eq!(usages[0].key, "password");
        assert_eq!(usages[0].value_path, vec![
            JsonPathKey::Field("database".to_string()),
            JsonPathKey::Field("connection".to_string()),
            JsonPathKey::Field("password".to_string()),
        ]);
    }

    #[test]
    fn test_find_secret_usages_in_array() {
        let value = serde_json::json!({
            "servers": [
                {"host": "server1", "password": {"$secret": "secret/data/servers/1:pass"}},
                {"host": "server2", "password": {"$secret": "secret/data/servers/2:pass"}}
            ]
        });
        let usages = find_secret_usages(&value).unwrap();
        assert_eq!(usages.len(), 2);
        assert_eq!(usages[0].path, "secret/data/servers/1");
        assert_eq!(usages[1].path, "secret/data/servers/2");
    }

    #[test]
    fn test_find_secret_usages_invalid_reference() {
        let value = serde_json::json!({
            "password": {"$secret": "no-colon-here"}
        });
        let result = find_secret_usages(&value);
        assert!(matches!(result, Err(SettingsError::InvalidSecretReference { .. })));
    }

    #[test]
    fn test_find_secret_usages_root_level_secret() {
        let value = serde_json::json!({"$secret": "path:key"});
        let usages = find_secret_usages(&value).unwrap();
        assert_eq!(usages.len(), 1);
        assert!(usages[0].value_path.is_empty());
    }

    #[test]
    fn test_secrets_service_version_starts_at_zero() {
        let service = SecretsService::new_without_vault();
        assert_eq!(service.version(), 0);
    }
}
