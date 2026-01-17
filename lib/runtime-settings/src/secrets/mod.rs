// lib/runtime-settings/src/secrets/mod.rs

pub mod resolver;

use crate::error::SettingsError;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};

pub use resolver::resolve_secrets;

/// Cached secret with metadata
struct CachedSecret {
    value: serde_json::Value,
    #[allow(dead_code)]
    lease_id: Option<String>,
    #[allow(dead_code)]
    lease_duration: Option<Duration>,
    #[allow(dead_code)]
    renewable: bool,
    #[allow(dead_code)]
    fetched_at: Instant,
}

pub struct SecretsService {
    client: Option<VaultClient>,
    cache: RwLock<HashMap<String, CachedSecret>>,
    #[allow(dead_code)]
    refresh_intervals: HashMap<String, Duration>,
}

impl SecretsService {
    /// Create without Vault (secrets will fail)
    pub fn new_without_vault() -> Self {
        Self {
            client: None,
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::default_refresh_intervals(),
        }
    }

    /// Create with Vault client
    pub fn new(client: VaultClient) -> Self {
        Self {
            client: Some(client),
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::default_refresh_intervals(),
        }
    }

    /// Create Vault client from environment
    pub fn from_env() -> Result<Self, SettingsError> {
        let address =
            std::env::var("VAULT_ADDR").unwrap_or_else(|_| "http://127.0.0.1:8200".to_string());
        let token = std::env::var("VAULT_TOKEN").ok();

        if token.is_none() {
            return Ok(Self::new_without_vault());
        }

        let settings = VaultClientSettingsBuilder::default()
            .address(&address)
            .token(token.unwrap())
            .build()
            .map_err(|e| SettingsError::Vault(e.to_string()))?;

        let client = VaultClient::new(settings).map_err(|e| SettingsError::Vault(e.to_string()))?;

        Ok(Self::new(client))
    }

    fn default_refresh_intervals() -> HashMap<String, Duration> {
        let mut intervals = HashMap::new();
        intervals.insert("kafka-certificates".to_string(), Duration::from_secs(600));
        intervals.insert("interservice-auth".to_string(), Duration::from_secs(60));
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

        // Fetch from Vault
        let secret: serde_json::Value = vaultrs::kv2::read(client, "secret", path)
            .await
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

    /// Refresh all cached secrets
    pub async fn refresh(&self) -> Result<(), SettingsError> {
        // TODO: Implement lease renewal and refresh logic
        Ok(())
    }
}
