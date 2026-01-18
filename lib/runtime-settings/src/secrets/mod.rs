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
    lease_duration: Option<Duration>,
    renewable: bool,
    fetched_at: Instant,
}

impl CachedSecret {
    fn needs_refresh(&self, threshold: f64) -> bool {
        match self.lease_duration {
            Some(duration) if self.renewable => {
                let elapsed = self.fetched_at.elapsed();
                let threshold_duration = Duration::from_secs_f64(duration.as_secs_f64() * threshold);
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

        for path in paths_to_refresh {
            match vaultrs::kv2::read::<serde_json::Value>(client, "secret", &path).await {
                Ok(secret) => {
                    let mut cache = self.cache.write().await;
                    cache.insert(
                        path.clone(),
                        CachedSecret {
                            value: secret,
                            lease_id: None,
                            lease_duration: None,
                            renewable: false,
                            fetched_at: Instant::now(),
                        },
                    );
                    tracing::debug!(path = %path, "Refreshed secret");
                }
                Err(e) => {
                    tracing::warn!(path = %path, error = %e, "Failed to refresh secret");
                }
            }
        }

        Ok(())
    }
}
