use crate::VaultError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MIN_REMAINING_TTL: Duration = Duration::from_secs(3600); // 1 hour

#[derive(Serialize, Deserialize)]
struct CachedToken {
    token: String,
    expires_at: u64, // Unix timestamp
}

pub struct OidcCache {
    cache_dir: PathBuf,
}

impl OidcCache {
    pub fn new() -> Option<Self> {
        directories::ProjectDirs::from("", "", "vault-client")
            .map(|dirs| Self {
                cache_dir: dirs.cache_dir().to_path_buf(),
            })
    }

    fn cache_key(vault_addr: &str, auth_method: &str, role: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(vault_addr);
        hasher.update(auth_method);
        hasher.update(role);
        format!("{:x}", hasher.finalize())
    }

    fn cache_path(&self, vault_addr: &str, auth_method: &str, role: &str) -> PathBuf {
        let key = Self::cache_key(vault_addr, auth_method, role);
        self.cache_dir.join(format!("{}.json", key))
    }

    /// Get cached token if valid for at least MIN_REMAINING_TTL
    pub fn get(&self, vault_addr: &str, auth_method: &str, role: &str) -> Option<String> {
        let path = self.cache_path(vault_addr, auth_method, role);
        let content = std::fs::read_to_string(&path).ok()?;
        let cached: CachedToken = serde_json::from_str(&content).ok()?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let remaining = cached.expires_at.saturating_sub(now);
        if remaining >= MIN_REMAINING_TTL.as_secs() {
            Some(cached.token)
        } else {
            None
        }
    }

    /// Store token with expiration
    pub fn set(
        &self,
        vault_addr: &str,
        auth_method: &str,
        role: &str,
        token: &str,
        ttl: Duration,
    ) -> Result<(), VaultError> {
        std::fs::create_dir_all(&self.cache_dir)?;

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + ttl.as_secs();

        let cached = CachedToken {
            token: token.to_string(),
            expires_at,
        };

        let path = self.cache_path(vault_addr, auth_method, role);
        let content = serde_json::to_string(&cached)?;
        std::fs::write(&path, content)?;

        Ok(())
    }

    /// Clear cached token
    pub fn clear(&self, vault_addr: &str, auth_method: &str, role: &str) {
        let path = self.cache_path(vault_addr, auth_method, role);
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cache_with_temp_dir() -> (OidcCache, TempDir) {
        let dir = TempDir::new().unwrap();
        let cache = OidcCache {
            cache_dir: dir.path().to_path_buf(),
        };
        (cache, dir)
    }

    #[test]
    fn test_cache_key_is_deterministic() {
        let key1 = OidcCache::cache_key("http://vault:8200", "oidc", "dev");
        let key2 = OidcCache::cache_key("http://vault:8200", "oidc", "dev");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_differs_for_different_inputs() {
        let key1 = OidcCache::cache_key("http://vault:8200", "oidc", "dev");
        let key2 = OidcCache::cache_key("http://vault:8200", "oidc", "prod");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_set_and_get_token() {
        let (cache, _dir) = cache_with_temp_dir();

        cache
            .set("http://vault:8200", "oidc", "dev", "my-token", Duration::from_secs(7200))
            .unwrap();

        let token = cache.get("http://vault:8200", "oidc", "dev");
        assert_eq!(token, Some("my-token".to_string()));
    }

    #[test]
    fn test_expired_token_not_returned() {
        let (cache, _dir) = cache_with_temp_dir();

        // Set token with 0 TTL (already expired)
        cache
            .set("http://vault:8200", "oidc", "dev", "expired", Duration::ZERO)
            .unwrap();

        let token = cache.get("http://vault:8200", "oidc", "dev");
        assert!(token.is_none());
    }

    #[test]
    fn test_clear_removes_token() {
        let (cache, _dir) = cache_with_temp_dir();

        cache
            .set("http://vault:8200", "oidc", "dev", "my-token", Duration::from_secs(7200))
            .unwrap();

        cache.clear("http://vault:8200", "oidc", "dev");

        let token = cache.get("http://vault:8200", "oidc", "dev");
        assert!(token.is_none());
    }
}
