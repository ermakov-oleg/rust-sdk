# vault-client Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create a Rust library for HashiCorp Vault with auto-detection of authentication method and background token renewal.

**Architecture:** VaultClient wraps vaultrs internally, adding TokenManager for automatic authentication and renewal. Auth methods (Token, K8s, OIDC) implement a common trait. Background tokio task handles token refresh at 75% TTL.

**Tech Stack:** vaultrs 0.7, tokio, serde, thiserror, chrono, directories, sha2, webbrowser, tracing

---

## Task 1: Create library scaffold

**Files:**
- Create: `lib/vault-client/Cargo.toml`
- Create: `lib/vault-client/src/lib.rs`
- Modify: `Cargo.toml` (workspace)

**Step 1: Create Cargo.toml**

```toml
[package]
name = "vault-client"
version = "0.1.0"
edition = "2024"

[dependencies]
vaultrs = "0.7"
tokio = { version = "1", features = ["sync", "time", "rt", "net"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
chrono = { version = "0.4", features = ["serde"] }
directories = "5"
sha2 = "0.10"
webbrowser = "1"
tracing = "0.1"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
wiremock = "0.6"
tempfile = "3"
```

**Step 2: Create lib.rs stub**

```rust
//! vault-client - Rust client for HashiCorp Vault
//!
//! Auto-detects authentication method:
//! 1. VAULT_TOKEN → static token
//! 2. KUBERNETES_SERVICE_HOST → K8s auth
//! 3. Otherwise → OIDC (local development)

mod error;
mod models;

pub use error::VaultError;
pub use models::{KvData, KvMetadata, KvVersion};
```

**Step 3: Add to workspace**

In root `Cargo.toml`, add `"lib/vault-client"` to members array.

**Step 4: Verify build**

Run: `cargo check -p vault-client`
Expected: Compilation errors (missing modules) - that's fine for now

**Step 5: Commit**

```bash
git add lib/vault-client Cargo.toml
git commit -m "chore(vault-client): add library scaffold"
```

---

## Task 2: Implement error types

**Files:**
- Create: `lib/vault-client/src/error.rs`
- Modify: `lib/vault-client/src/lib.rs`

**Step 1: Write error.rs**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault not detected: VAULT_ADDR not set")]
    VaultNotDetected,

    #[error("Secret not found: {path}")]
    SecretNotFound { path: String },

    #[error("Vault client error ({status}): {message}")]
    ClientError {
        status: u16,
        message: String,
        response_data: Option<serde_json::Value>,
    },

    #[error("Vault request error: {0}")]
    RequestError(String),

    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("OIDC authentication failed: {0}")]
    OidcError(String),

    #[error("Kubernetes auth failed: {0}")]
    KubernetesError(String),

    #[error("Token expired and renewal failed")]
    TokenExpired,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
```

**Step 2: Update lib.rs**

```rust
mod error;
mod models;

pub use error::VaultError;
```

**Step 3: Verify build**

Run: `cargo check -p vault-client`
Expected: Error about missing models module

**Step 4: Commit**

```bash
git add lib/vault-client/src/error.rs lib/vault-client/src/lib.rs
git commit -m "feat(vault-client): add error types"
```

---

## Task 3: Implement data models

**Files:**
- Create: `lib/vault-client/src/models.rs`
- Modify: `lib/vault-client/src/lib.rs`

**Step 1: Write models.rs**

```rust
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
```

**Step 2: Update lib.rs exports**

```rust
mod error;
mod models;

pub use error::VaultError;
pub use models::{KvData, KvMetadata, KvVersion};
```

**Step 3: Run tests**

Run: `cargo test -p vault-client`
Expected: 2 tests pass

**Step 4: Commit**

```bash
git add lib/vault-client/src/models.rs lib/vault-client/src/lib.rs
git commit -m "feat(vault-client): add data models"
```

---

## Task 4: Implement auth module scaffold and TokenInfo

**Files:**
- Create: `lib/vault-client/src/auth/mod.rs`
- Create: `lib/vault-client/src/auth/token_info.rs`
- Modify: `lib/vault-client/src/lib.rs`

**Step 1: Create auth/token_info.rs**

```rust
use std::time::{Duration, Instant};

/// Token information from authentication
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub token: String,
    pub lease_duration: Duration,
    pub renewable: bool,
    pub obtained_at: Instant,
}

impl TokenInfo {
    pub fn new(token: String, lease_duration: Duration, renewable: bool) -> Self {
        Self {
            token,
            lease_duration,
            renewable,
            obtained_at: Instant::now(),
        }
    }

    /// Static token (never expires)
    pub fn static_token(token: String) -> Self {
        Self {
            token,
            lease_duration: Duration::ZERO,
            renewable: false,
            obtained_at: Instant::now(),
        }
    }

    /// Check if token needs refresh (at threshold % of lease)
    pub fn needs_refresh(&self, threshold: f64) -> bool {
        if self.lease_duration.is_zero() {
            return false; // Static token
        }
        let elapsed = self.obtained_at.elapsed();
        let threshold_duration = Duration::from_secs_f64(self.lease_duration.as_secs_f64() * threshold);
        elapsed >= threshold_duration
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        if self.lease_duration.is_zero() {
            return false;
        }
        self.obtained_at.elapsed() >= self.lease_duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_token_never_expires() {
        let token = TokenInfo::static_token("test".to_string());
        assert!(!token.needs_refresh(0.75));
        assert!(!token.is_expired());
    }

    #[test]
    fn test_token_needs_refresh_at_threshold() {
        let mut token = TokenInfo::new("test".to_string(), Duration::from_secs(100), true);
        // Simulate time passing
        token.obtained_at = Instant::now() - Duration::from_secs(80);
        assert!(token.needs_refresh(0.75));
    }

    #[test]
    fn test_token_not_expired_before_lease() {
        let mut token = TokenInfo::new("test".to_string(), Duration::from_secs(100), true);
        token.obtained_at = Instant::now() - Duration::from_secs(50);
        assert!(!token.is_expired());
    }
}
```

**Step 2: Create auth/mod.rs**

```rust
mod token_info;

pub use token_info::TokenInfo;

use crate::VaultError;
use async_trait::async_trait;

/// Trait for authentication methods
#[async_trait]
pub trait AuthMethod: Send + Sync {
    /// Perform initial authentication
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError>;

    /// Whether this method supports token renewal via renew-self
    fn supports_renewal(&self) -> bool;
}
```

**Step 3: Update lib.rs**

```rust
mod auth;
mod error;
mod models;

pub use error::VaultError;
pub use models::{KvData, KvMetadata, KvVersion};

// Re-export for advanced usage
pub use auth::TokenInfo;
```

**Step 4: Add async-trait dependency**

Add to `lib/vault-client/Cargo.toml`:
```toml
async-trait = "0.1"
```

**Step 5: Run tests**

Run: `cargo test -p vault-client`
Expected: 5 tests pass (2 models + 3 token_info)

**Step 6: Commit**

```bash
git add lib/vault-client/src/auth lib/vault-client/Cargo.toml lib/vault-client/src/lib.rs
git commit -m "feat(vault-client): add auth module with TokenInfo"
```

---

## Task 5: Implement static token auth

**Files:**
- Create: `lib/vault-client/src/auth/token.rs`
- Modify: `lib/vault-client/src/auth/mod.rs`

**Step 1: Create auth/token.rs**

```rust
use super::{AuthMethod, TokenInfo};
use crate::VaultError;
use async_trait::async_trait;

/// Static token authentication
pub struct StaticTokenAuth {
    token: String,
}

impl StaticTokenAuth {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl AuthMethod for StaticTokenAuth {
    async fn authenticate(&self, _base_url: &str) -> Result<TokenInfo, VaultError> {
        Ok(TokenInfo::static_token(self.token.clone()))
    }

    fn supports_renewal(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_token_auth() {
        let auth = StaticTokenAuth::new("my-token".to_string());
        let token_info = auth.authenticate("http://vault:8200").await.unwrap();
        assert_eq!(token_info.token, "my-token");
        assert!(!auth.supports_renewal());
    }
}
```

**Step 2: Update auth/mod.rs**

```rust
mod token;
mod token_info;

pub use token::StaticTokenAuth;
pub use token_info::TokenInfo;

use crate::VaultError;
use async_trait::async_trait;

#[async_trait]
pub trait AuthMethod: Send + Sync {
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError>;
    fn supports_renewal(&self) -> bool;
}
```

**Step 3: Run tests**

Run: `cargo test -p vault-client`
Expected: 6 tests pass

**Step 4: Commit**

```bash
git add lib/vault-client/src/auth/token.rs lib/vault-client/src/auth/mod.rs
git commit -m "feat(vault-client): add static token auth"
```

---

## Task 6: Implement Kubernetes auth

**Files:**
- Create: `lib/vault-client/src/auth/kubernetes.rs`
- Modify: `lib/vault-client/src/auth/mod.rs`

**Step 1: Create auth/kubernetes.rs**

```rust
use super::{AuthMethod, TokenInfo};
use crate::VaultError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_JWT_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";

/// Kubernetes authentication
pub struct KubernetesAuth {
    pub auth_method: String,
    pub role: String,
    pub jwt_path: String,
}

impl KubernetesAuth {
    pub fn new(auth_method: String, role: String) -> Self {
        Self {
            auth_method,
            role,
            jwt_path: DEFAULT_JWT_PATH.to_string(),
        }
    }

    pub fn with_jwt_path(mut self, path: String) -> Self {
        self.jwt_path = path;
        self
    }

    fn read_jwt(&self) -> Result<String, VaultError> {
        std::fs::read_to_string(&self.jwt_path)
            .map(|s| s.trim().to_string())
            .map_err(|e| VaultError::KubernetesError(format!("Failed to read JWT from {}: {}", self.jwt_path, e)))
    }
}

#[derive(Serialize)]
struct LoginRequest {
    jwt: String,
    role: String,
}

#[derive(Deserialize)]
struct LoginResponse {
    auth: AuthData,
}

#[derive(Deserialize)]
struct AuthData {
    client_token: String,
    lease_duration: u64,
    renewable: bool,
}

#[async_trait]
impl AuthMethod for KubernetesAuth {
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError> {
        let jwt = self.read_jwt()?;

        let client = reqwest::Client::new();
        let url = format!("{}/v1/auth/{}/login", base_url, self.auth_method);

        let response = client
            .post(&url)
            .json(&LoginRequest {
                jwt,
                role: self.role.clone(),
            })
            .send()
            .await
            .map_err(|e| VaultError::RequestError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(VaultError::ClientError {
                status,
                message: body,
                response_data: None,
            });
        }

        let login: LoginResponse = response
            .json()
            .await
            .map_err(|e| VaultError::AuthError(format!("Invalid response: {}", e)))?;

        Ok(TokenInfo::new(
            login.auth.client_token,
            Duration::from_secs(login.auth.lease_duration),
            login.auth.renewable,
        ))
    }

    fn supports_renewal(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_read_jwt_from_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "my-jwt-token").unwrap();

        let auth = KubernetesAuth::new("kubernetes".to_string(), "app".to_string())
            .with_jwt_path(file.path().to_str().unwrap().to_string());

        let jwt = auth.read_jwt().unwrap();
        assert_eq!(jwt, "my-jwt-token");
    }

    #[test]
    fn test_read_jwt_missing_file() {
        let auth = KubernetesAuth::new("kubernetes".to_string(), "app".to_string())
            .with_jwt_path("/nonexistent/path".to_string());

        let result = auth.read_jwt();
        assert!(matches!(result, Err(VaultError::KubernetesError(_))));
    }
}
```

**Step 2: Update auth/mod.rs**

```rust
mod kubernetes;
mod token;
mod token_info;

pub use kubernetes::KubernetesAuth;
pub use token::StaticTokenAuth;
pub use token_info::TokenInfo;

use crate::VaultError;
use async_trait::async_trait;

#[async_trait]
pub trait AuthMethod: Send + Sync {
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError>;
    fn supports_renewal(&self) -> bool;
}
```

**Step 3: Run tests**

Run: `cargo test -p vault-client`
Expected: 8 tests pass

**Step 4: Commit**

```bash
git add lib/vault-client/src/auth/kubernetes.rs lib/vault-client/src/auth/mod.rs
git commit -m "feat(vault-client): add kubernetes auth"
```

---

## Task 7: Implement OIDC disk cache

**Files:**
- Create: `lib/vault-client/src/auth/oidc_cache.rs`
- Modify: `lib/vault-client/src/auth/mod.rs`

**Step 1: Create auth/oidc_cache.rs**

```rust
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
```

**Step 2: Update auth/mod.rs**

```rust
mod kubernetes;
mod oidc_cache;
mod token;
mod token_info;

pub use kubernetes::KubernetesAuth;
pub use oidc_cache::OidcCache;
pub use token::StaticTokenAuth;
pub use token_info::TokenInfo;

use crate::VaultError;
use async_trait::async_trait;

#[async_trait]
pub trait AuthMethod: Send + Sync {
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError>;
    fn supports_renewal(&self) -> bool;
}
```

**Step 3: Run tests**

Run: `cargo test -p vault-client`
Expected: 13 tests pass

**Step 4: Commit**

```bash
git add lib/vault-client/src/auth/oidc_cache.rs lib/vault-client/src/auth/mod.rs
git commit -m "feat(vault-client): add OIDC token disk cache"
```

---

## Task 8: Implement OIDC auth

**Files:**
- Create: `lib/vault-client/src/auth/oidc.rs`
- Modify: `lib/vault-client/src/auth/mod.rs`

**Step 1: Create auth/oidc.rs**

```rust
use super::{AuthMethod, OidcCache, TokenInfo};
use crate::VaultError;
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

const CALLBACK_PORT: u16 = 8250;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// OIDC authentication (for local development)
pub struct OidcAuth {
    pub auth_method: String,
    pub role: String,
    cache: Option<OidcCache>,
}

impl OidcAuth {
    pub fn new(auth_method: String, role: String) -> Self {
        Self {
            auth_method,
            role,
            cache: OidcCache::new(),
        }
    }

    async fn get_auth_url(&self, base_url: &str) -> Result<(String, String), VaultError> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/v1/auth/{}/oidc/auth_url",
            base_url, self.auth_method
        );

        let response = client
            .post(&url)
            .json(&serde_json::json!({
                "role": self.role,
                "redirect_uri": format!("http://localhost:{}/oidc/callback", CALLBACK_PORT)
            }))
            .send()
            .await
            .map_err(|e| VaultError::RequestError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(VaultError::ClientError {
                status,
                message: body,
                response_data: None,
            });
        }

        #[derive(Deserialize)]
        struct AuthUrlResponse {
            data: AuthUrlData,
        }
        #[derive(Deserialize)]
        struct AuthUrlData {
            auth_url: String,
            state: String,
        }

        let resp: AuthUrlResponse = response
            .json()
            .await
            .map_err(|e| VaultError::OidcError(format!("Invalid auth_url response: {}", e)))?;

        Ok((resp.data.auth_url, resp.data.state))
    }

    async fn wait_for_callback(&self, expected_state: &str) -> Result<(String, String), VaultError> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", CALLBACK_PORT))
            .await
            .map_err(|e| VaultError::OidcError(format!("Failed to bind callback port: {}", e)))?;

        let result = tokio::time::timeout(CALLBACK_TIMEOUT, async {
            let (mut stream, _) = listener.accept().await?;

            let mut reader = BufReader::new(&mut stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).await?;

            // Parse: GET /oidc/callback?state=...&code=... HTTP/1.1
            let path = request_line
                .split_whitespace()
                .nth(1)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid request"))?;

            let query = path
                .split('?')
                .nth(1)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "No query string"))?;

            let mut state = None;
            let mut code = None;

            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    match key {
                        "state" => state = Some(value.to_string()),
                        "code" => code = Some(value.to_string()),
                        _ => {}
                    }
                }
            }

            // Send response
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Authentication successful!</h1><p>You can close this window.</p></body></html>";
            stream.write_all(response.as_bytes()).await?;

            Ok::<_, std::io::Error>((
                state.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing state"))?,
                code.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing code"))?,
            ))
        })
        .await
        .map_err(|_| VaultError::OidcError("Callback timeout".to_string()))?
        .map_err(|e| VaultError::OidcError(format!("Callback error: {}", e)))?;

        let (state, code) = result;
        if state != expected_state {
            return Err(VaultError::OidcError("State mismatch".to_string()));
        }

        Ok((state, code))
    }

    async fn exchange_code(
        &self,
        base_url: &str,
        state: &str,
        code: &str,
    ) -> Result<TokenInfo, VaultError> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/v1/auth/{}/oidc/callback?state={}&code={}",
            base_url, self.auth_method, state, code
        );

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| VaultError::RequestError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(VaultError::ClientError {
                status,
                message: body,
                response_data: None,
            });
        }

        #[derive(Deserialize)]
        struct CallbackResponse {
            auth: AuthData,
        }
        #[derive(Deserialize)]
        struct AuthData {
            client_token: String,
            lease_duration: u64,
            renewable: bool,
        }

        let resp: CallbackResponse = response
            .json()
            .await
            .map_err(|e| VaultError::OidcError(format!("Invalid callback response: {}", e)))?;

        Ok(TokenInfo::new(
            resp.auth.client_token,
            Duration::from_secs(resp.auth.lease_duration),
            resp.auth.renewable,
        ))
    }
}

#[async_trait]
impl AuthMethod for OidcAuth {
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError> {
        // Check cache first
        if let Some(ref cache) = self.cache {
            if let Some(token) = cache.get(base_url, &self.auth_method, &self.role) {
                tracing::debug!("Using cached OIDC token");
                return Ok(TokenInfo::static_token(token));
            }
        }

        // Get auth URL
        let (auth_url, state) = self.get_auth_url(base_url).await?;

        // Open browser
        tracing::info!("Opening browser for OIDC authentication...");
        if webbrowser::open(&auth_url).is_err() {
            tracing::warn!("Failed to open browser. Please visit: {}", auth_url);
        }

        // Wait for callback
        let (state, code) = self.wait_for_callback(&state).await?;

        // Exchange code for token
        let token_info = self.exchange_code(base_url, &state, &code).await?;

        // Cache token
        if let Some(ref cache) = self.cache {
            if let Err(e) = cache.set(
                base_url,
                &self.auth_method,
                &self.role,
                &token_info.token,
                token_info.lease_duration,
            ) {
                tracing::warn!("Failed to cache OIDC token: {}", e);
            }
        }

        Ok(token_info)
    }

    fn supports_renewal(&self) -> bool {
        true
    }
}
```

**Step 2: Update auth/mod.rs**

```rust
mod kubernetes;
mod oidc;
mod oidc_cache;
mod token;
mod token_info;

pub use kubernetes::KubernetesAuth;
pub use oidc::OidcAuth;
pub use oidc_cache::OidcCache;
pub use token::StaticTokenAuth;
pub use token_info::TokenInfo;

use crate::VaultError;
use async_trait::async_trait;

#[async_trait]
pub trait AuthMethod: Send + Sync {
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError>;
    fn supports_renewal(&self) -> bool;
}
```

**Step 3: Run tests**

Run: `cargo test -p vault-client`
Expected: 13 tests pass (no new tests for OIDC - requires real Vault)

**Step 4: Commit**

```bash
git add lib/vault-client/src/auth/oidc.rs lib/vault-client/src/auth/mod.rs
git commit -m "feat(vault-client): add OIDC auth with browser flow"
```

---

## Task 9: Implement TokenManager with background renewal

**Files:**
- Create: `lib/vault-client/src/auth/manager.rs`
- Modify: `lib/vault-client/src/auth/mod.rs`

**Step 1: Create auth/manager.rs**

```rust
use super::{AuthMethod, TokenInfo};
use crate::VaultError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const DEFAULT_REFRESH_THRESHOLD: f64 = 0.75;
const DEFAULT_MIN_RENEWAL_DURATION: Duration = Duration::from_secs(300);
const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(10);

pub struct TokenManagerConfig {
    pub refresh_threshold: f64,
    pub min_renewal_duration: Duration,
    pub retry_interval: Duration,
}

impl Default for TokenManagerConfig {
    fn default() -> Self {
        Self {
            refresh_threshold: DEFAULT_REFRESH_THRESHOLD,
            min_renewal_duration: DEFAULT_MIN_RENEWAL_DURATION,
            retry_interval: DEFAULT_RETRY_INTERVAL,
        }
    }
}

pub struct TokenManager {
    base_url: String,
    auth_method: Arc<dyn AuthMethod>,
    token: Arc<RwLock<TokenInfo>>,
    config: TokenManagerConfig,
}

impl TokenManager {
    pub async fn new(
        base_url: String,
        auth_method: Arc<dyn AuthMethod>,
        config: TokenManagerConfig,
    ) -> Result<Self, VaultError> {
        let token_info = auth_method.authenticate(&base_url).await?;

        let manager = Self {
            base_url,
            auth_method,
            token: Arc::new(RwLock::new(token_info)),
            config,
        };

        manager.start_renewal_task();

        Ok(manager)
    }

    pub async fn get_token(&self) -> String {
        self.token.read().await.token.clone()
    }

    fn start_renewal_task(&self) {
        let token = Arc::clone(&self.token);
        let auth_method = Arc::clone(&self.auth_method);
        let base_url = self.base_url.clone();
        let config = TokenManagerConfig {
            refresh_threshold: self.config.refresh_threshold,
            min_renewal_duration: self.config.min_renewal_duration,
            retry_interval: self.config.retry_interval,
        };

        tokio::spawn(async move {
            loop {
                let (needs_refresh, sleep_duration) = {
                    let token_info = token.read().await;

                    if token_info.lease_duration.is_zero() {
                        // Static token, never refresh
                        break;
                    }

                    if token_info.lease_duration < config.min_renewal_duration {
                        // Token duration too short for renewal
                        break;
                    }

                    let needs = token_info.needs_refresh(config.refresh_threshold);
                    let until_refresh = if needs {
                        Duration::ZERO
                    } else {
                        let threshold_time = Duration::from_secs_f64(
                            token_info.lease_duration.as_secs_f64() * config.refresh_threshold,
                        );
                        threshold_time.saturating_sub(token_info.obtained_at.elapsed())
                    };

                    (needs, until_refresh)
                };

                if !needs_refresh {
                    tokio::time::sleep(sleep_duration).await;
                    continue;
                }

                // Try to renew
                match Self::renew_token(&base_url, &token).await {
                    Ok(()) => {
                        tracing::debug!("Token renewed successfully");
                    }
                    Err(VaultError::ClientError { status, .. }) if status >= 400 && status < 500 => {
                        // 4xx error - need to re-authenticate
                        tracing::info!("Token renewal failed with 4xx, re-authenticating");
                        match auth_method.authenticate(&base_url).await {
                            Ok(new_token) => {
                                let mut t = token.write().await;
                                *t = new_token;
                                tracing::debug!("Re-authenticated successfully");
                            }
                            Err(e) => {
                                tracing::error!("Re-authentication failed: {}", e);
                                tokio::time::sleep(config.retry_interval).await;
                            }
                        }
                    }
                    Err(e) => {
                        // 5xx or network error - retry later
                        tracing::warn!("Token renewal failed: {}, retrying in {:?}", e, config.retry_interval);
                        tokio::time::sleep(config.retry_interval).await;
                    }
                }
            }
        });
    }

    async fn renew_token(base_url: &str, token: &Arc<RwLock<TokenInfo>>) -> Result<(), VaultError> {
        let current_token = token.read().await.token.clone();

        let client = reqwest::Client::new();
        let url = format!("{}/v1/auth/token/renew-self", base_url);

        let response = client
            .post(&url)
            .header("X-Vault-Token", &current_token)
            .send()
            .await
            .map_err(|e| VaultError::RequestError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(VaultError::ClientError {
                status,
                message: body,
                response_data: None,
            });
        }

        #[derive(serde::Deserialize)]
        struct RenewResponse {
            auth: AuthData,
        }
        #[derive(serde::Deserialize)]
        struct AuthData {
            client_token: String,
            lease_duration: u64,
            renewable: bool,
        }

        let resp: RenewResponse = response
            .json()
            .await
            .map_err(|e| VaultError::AuthError(format!("Invalid renewal response: {}", e)))?;

        let mut t = token.write().await;
        *t = TokenInfo::new(
            resp.auth.client_token,
            Duration::from_secs(resp.auth.lease_duration),
            resp.auth.renewable,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StaticTokenAuth;

    #[tokio::test]
    async fn test_token_manager_with_static_token() {
        let auth = Arc::new(StaticTokenAuth::new("my-token".to_string()));
        let manager = TokenManager::new(
            "http://vault:8200".to_string(),
            auth,
            TokenManagerConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(manager.get_token().await, "my-token");
    }
}
```

**Step 2: Update auth/mod.rs**

```rust
mod kubernetes;
mod manager;
mod oidc;
mod oidc_cache;
mod token;
mod token_info;

pub use kubernetes::KubernetesAuth;
pub use manager::{TokenManager, TokenManagerConfig};
pub use oidc::OidcAuth;
pub use oidc_cache::OidcCache;
pub use token::StaticTokenAuth;
pub use token_info::TokenInfo;

use crate::VaultError;
use async_trait::async_trait;

#[async_trait]
pub trait AuthMethod: Send + Sync {
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError>;
    fn supports_renewal(&self) -> bool;
}
```

**Step 3: Run tests**

Run: `cargo test -p vault-client`
Expected: 14 tests pass

**Step 4: Commit**

```bash
git add lib/vault-client/src/auth/manager.rs lib/vault-client/src/auth/mod.rs
git commit -m "feat(vault-client): add TokenManager with background renewal"
```

---

## Task 10: Implement VaultClientBuilder and VaultClient

**Files:**
- Create: `lib/vault-client/src/client.rs`
- Modify: `lib/vault-client/src/lib.rs`

**Step 1: Create client.rs**

```rust
use crate::auth::{
    AuthMethod, KubernetesAuth, OidcAuth, StaticTokenAuth, TokenManager, TokenManagerConfig,
};
use crate::error::VaultError;
use crate::models::{KvData, KvMetadata, KvVersion};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_K8S_AUTH_METHOD: &str = "kubernetes";
const DEFAULT_OIDC_AUTH_METHOD: &str = "oidc";
const DEFAULT_ROLE: &str = "app";

pub struct VaultClientBuilder {
    base_url: Option<String>,
    token: Option<String>,
    k8s_auth_method: Option<String>,
    oidc_auth_method: Option<String>,
    role: Option<String>,
    application_name: Option<String>,
    renewable_token_min_duration: Duration,
    retry_interval: Duration,
}

impl Default for VaultClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultClientBuilder {
    pub fn new() -> Self {
        Self {
            base_url: None,
            token: None,
            k8s_auth_method: None,
            oidc_auth_method: None,
            role: None,
            application_name: None,
            renewable_token_min_duration: Duration::from_secs(300),
            retry_interval: Duration::from_secs(10),
        }
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn k8s_auth_method(mut self, method: impl Into<String>) -> Self {
        self.k8s_auth_method = Some(method.into());
        self
    }

    pub fn oidc_auth_method(mut self, method: impl Into<String>) -> Self {
        self.oidc_auth_method = Some(method.into());
        self
    }

    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }

    pub fn renewable_token_min_duration(mut self, duration: Duration) -> Self {
        self.renewable_token_min_duration = duration;
        self
    }

    pub fn retry_interval(mut self, duration: Duration) -> Self {
        self.retry_interval = duration;
        self
    }

    /// Build from environment variables with overrides
    fn resolve_config(&self) -> Result<ResolvedConfig, VaultError> {
        let base_url = self
            .base_url
            .clone()
            .or_else(|| std::env::var("VAULT_ADDR").ok())
            .ok_or(VaultError::VaultNotDetected)?;

        let token = self
            .token
            .clone()
            .or_else(|| std::env::var("VAULT_TOKEN").ok());

        let k8s_auth_method = self
            .k8s_auth_method
            .clone()
            .or_else(|| std::env::var("VAULT_AUTH_METHOD").ok())
            .unwrap_or_else(|| DEFAULT_K8S_AUTH_METHOD.to_string());

        let oidc_auth_method = self
            .oidc_auth_method
            .clone()
            .unwrap_or_else(|| DEFAULT_OIDC_AUTH_METHOD.to_string());

        let role = self
            .role
            .clone()
            .or_else(|| std::env::var("VAULT_ROLE_ID").ok())
            .unwrap_or_else(|| DEFAULT_ROLE.to_string());

        let k8s_jwt_path = std::env::var("K8S_JWT_TOKEN_PATH")
            .unwrap_or_else(|_| "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string());

        let is_kubernetes = std::env::var("KUBERNETES_SERVICE_HOST").is_ok();

        Ok(ResolvedConfig {
            base_url,
            token,
            k8s_auth_method,
            oidc_auth_method,
            role,
            k8s_jwt_path,
            is_kubernetes,
            application_name: self.application_name.clone(),
            renewable_token_min_duration: self.renewable_token_min_duration,
            retry_interval: self.retry_interval,
        })
    }

    pub async fn build(self) -> Result<VaultClient, VaultError> {
        let config = self.resolve_config()?;

        // Determine auth method
        let auth_method: Arc<dyn AuthMethod> = if let Some(token) = config.token {
            // Priority 1: Static token
            Arc::new(StaticTokenAuth::new(token))
        } else if config.is_kubernetes {
            // Priority 2: Kubernetes
            Arc::new(
                KubernetesAuth::new(config.k8s_auth_method, config.role)
                    .with_jwt_path(config.k8s_jwt_path),
            )
        } else {
            // Priority 3: OIDC
            Arc::new(OidcAuth::new(config.oidc_auth_method, config.role))
        };

        let token_manager = TokenManager::new(
            config.base_url.clone(),
            auth_method,
            TokenManagerConfig {
                refresh_threshold: 0.75,
                min_renewal_duration: config.renewable_token_min_duration,
                retry_interval: config.retry_interval,
            },
        )
        .await?;

        Ok(VaultClient {
            base_url: config.base_url,
            token_manager,
            application_name: config.application_name,
        })
    }
}

struct ResolvedConfig {
    base_url: String,
    token: Option<String>,
    k8s_auth_method: String,
    oidc_auth_method: String,
    role: String,
    k8s_jwt_path: String,
    is_kubernetes: bool,
    application_name: Option<String>,
    renewable_token_min_duration: Duration,
    retry_interval: Duration,
}

pub struct VaultClient {
    base_url: String,
    token_manager: TokenManager,
    application_name: Option<String>,
}

impl VaultClient {
    /// Create client from environment variables with auto-detection
    pub async fn from_env() -> Result<Self, VaultError> {
        VaultClientBuilder::new().build().await
    }

    /// Get builder for custom configuration
    pub fn builder() -> VaultClientBuilder {
        VaultClientBuilder::new()
    }

    /// Read KV v2 secret
    pub async fn kv_read(&self, mount: &str, path: &str) -> Result<KvData, VaultError> {
        let url = format!("{}/v1/{}/data/{}", self.base_url, mount, path);
        let token = self.token_manager.get_token().await;

        let client = reqwest::Client::new();
        let mut request = client.get(&url).header("X-Vault-Token", token);

        if let Some(ref app_name) = self.application_name {
            request = request.header("User-Agent", app_name);
        }

        let response = request
            .send()
            .await
            .map_err(|e| VaultError::RequestError(e.to_string()))?;

        if response.status().as_u16() == 404 {
            return Err(VaultError::SecretNotFound {
                path: path.to_string(),
            });
        }

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(VaultError::ClientError {
                status,
                message: body,
                response_data: None,
            });
        }

        #[derive(serde::Deserialize)]
        struct KvResponse {
            data: KvResponseData,
        }

        #[derive(serde::Deserialize)]
        struct KvResponseData {
            data: HashMap<String, serde_json::Value>,
            metadata: KvVersionResponse,
        }

        #[derive(serde::Deserialize)]
        struct KvVersionResponse {
            version: u64,
            created_time: String,
            deletion_time: Option<String>,
            #[serde(default)]
            destroyed: bool,
        }

        let resp: KvResponse = response
            .json()
            .await
            .map_err(|e| VaultError::RequestError(format!("Invalid response: {}", e)))?;

        Ok(KvData {
            data: resp.data.data,
            metadata: KvVersion {
                version: resp.data.metadata.version,
                created_time: resp
                    .data
                    .metadata
                    .created_time
                    .parse()
                    .map_err(|e| VaultError::RequestError(format!("Invalid timestamp: {}", e)))?,
                deletion_time: resp
                    .data
                    .metadata
                    .deletion_time
                    .and_then(|s| s.parse().ok()),
                destroyed: resp.data.metadata.destroyed,
            },
        })
    }

    /// Read KV v2 secret metadata
    pub async fn kv_metadata(&self, mount: &str, path: &str) -> Result<KvMetadata, VaultError> {
        let url = format!("{}/v1/{}/metadata/{}", self.base_url, mount, path);
        let token = self.token_manager.get_token().await;

        let client = reqwest::Client::new();
        let mut request = client.get(&url).header("X-Vault-Token", token);

        if let Some(ref app_name) = self.application_name {
            request = request.header("User-Agent", app_name);
        }

        let response = request
            .send()
            .await
            .map_err(|e| VaultError::RequestError(e.to_string()))?;

        if response.status().as_u16() == 404 {
            return Err(VaultError::SecretNotFound {
                path: path.to_string(),
            });
        }

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(VaultError::ClientError {
                status,
                message: body,
                response_data: None,
            });
        }

        #[derive(serde::Deserialize)]
        struct MetadataResponse {
            data: MetadataData,
        }

        #[derive(serde::Deserialize)]
        struct MetadataData {
            created_time: String,
            custom_metadata: Option<HashMap<String, String>>,
            versions: HashMap<String, VersionInfo>,
        }

        #[derive(serde::Deserialize)]
        struct VersionInfo {
            created_time: String,
            deletion_time: Option<String>,
            #[serde(default)]
            destroyed: bool,
        }

        let resp: MetadataResponse = response
            .json()
            .await
            .map_err(|e| VaultError::RequestError(format!("Invalid response: {}", e)))?;

        let mut versions: Vec<KvVersion> = resp
            .data
            .versions
            .into_iter()
            .map(|(version_str, info)| {
                Ok(KvVersion {
                    version: version_str
                        .parse()
                        .map_err(|_| VaultError::RequestError("Invalid version".to_string()))?,
                    created_time: info
                        .created_time
                        .parse()
                        .map_err(|e| VaultError::RequestError(format!("Invalid timestamp: {}", e)))?,
                    deletion_time: info.deletion_time.and_then(|s| s.parse().ok()),
                    destroyed: info.destroyed,
                })
            })
            .collect::<Result<Vec<_>, VaultError>>()?;

        versions.sort_by_key(|v| v.version);

        Ok(KvMetadata {
            created_time: resp
                .data
                .created_time
                .parse()
                .map_err(|e| VaultError::RequestError(format!("Invalid timestamp: {}", e)))?,
            custom_metadata: resp.data.custom_metadata,
            versions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = VaultClientBuilder::new();
        assert!(builder.base_url.is_none());
        assert!(builder.token.is_none());
    }

    #[test]
    fn test_builder_chain() {
        let builder = VaultClientBuilder::new()
            .base_url("http://vault:8200")
            .token("my-token")
            .role("admin");

        assert_eq!(builder.base_url, Some("http://vault:8200".to_string()));
        assert_eq!(builder.token, Some("my-token".to_string()));
        assert_eq!(builder.role, Some("admin".to_string()));
    }
}
```

**Step 2: Update lib.rs**

```rust
//! vault-client - Rust client for HashiCorp Vault
//!
//! Auto-detects authentication method:
//! 1. VAULT_TOKEN → static token
//! 2. KUBERNETES_SERVICE_HOST → K8s auth
//! 3. Otherwise → OIDC (local development)
//!
//! # Example
//!
//! ```ignore
//! use vault_client::VaultClient;
//!
//! let client = VaultClient::from_env().await?;
//! let secret = client.kv_read("secret", "my/path").await?;
//! println!("{:?}", secret.data);
//! ```

mod auth;
mod client;
mod error;
mod models;

pub use client::{VaultClient, VaultClientBuilder};
pub use error::VaultError;
pub use models::{KvData, KvMetadata, KvVersion};

// Re-export for advanced usage
pub use auth::TokenInfo;
```

**Step 3: Run tests**

Run: `cargo test -p vault-client`
Expected: 16 tests pass

**Step 4: Commit**

```bash
git add lib/vault-client/src/client.rs lib/vault-client/src/lib.rs
git commit -m "feat(vault-client): add VaultClient and VaultClientBuilder"
```

---

## Task 11: Integrate vault-client into runtime-settings

**Files:**
- Modify: `lib/runtime-settings/Cargo.toml`
- Modify: `lib/runtime-settings/src/settings.rs`
- Modify: `lib/runtime-settings/src/secrets/mod.rs`

**Step 1: Update runtime-settings Cargo.toml**

Replace `vaultrs` dependency with `vault-client`:

```toml
# Remove: vaultrs = "0.7"
# Add:
vault-client = { path = "../vault-client" }
```

**Step 2: Update secrets/mod.rs**

Replace vaultrs imports and usage:

```rust
// Remove:
// use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};

// Add:
use vault_client::VaultClient;

// Update SecretsService struct:
pub struct SecretsService {
    client: Option<VaultClient>,  // Now vault_client::VaultClient
    cache: RwLock<HashMap<String, CachedSecret>>,
    refresh_intervals: HashMap<String, Duration>,
    version: AtomicU64,
}

// Remove from_env() method - client now passed from outside

impl SecretsService {
    pub fn new_without_vault() -> Self {
        Self {
            client: None,
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::load_refresh_intervals(),
            version: AtomicU64::new(0),
        }
    }

    pub fn new(client: VaultClient) -> Self {
        Self {
            client: Some(client),
            cache: RwLock::new(HashMap::new()),
            refresh_intervals: Self::load_refresh_intervals(),
            version: AtomicU64::new(0),
        }
    }

    // Update get() method to use vault_client API:
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
            .kv_read("secret", path)
            .await
            .map_err(|e| SettingsError::Vault(e.to_string()))?;

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

    // Update refresh() similarly
}
```

**Step 3: Update settings.rs builder**

Add vault_client method to RuntimeSettingsBuilder:

```rust
use vault_client::VaultClient;

pub struct RuntimeSettingsBuilder {
    // ... existing fields ...
    vault_client: Option<VaultClient>,
}

impl RuntimeSettingsBuilder {
    pub fn vault_client(mut self, client: VaultClient) -> Self {
        self.vault_client = Some(client);
        self
    }

    pub async fn build(self) -> Result<RuntimeSettings, SettingsError> {
        // ...
        let secrets = match self.vault_client {
            Some(client) => SecretsService::new(client),
            None => SecretsService::new_without_vault(),
        };
        // ...
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p runtime-settings`
Expected: All existing tests pass

**Step 5: Commit**

```bash
git add lib/runtime-settings/Cargo.toml lib/runtime-settings/src/secrets/mod.rs lib/runtime-settings/src/settings.rs
git commit -m "refactor(runtime-settings): use vault-client instead of vaultrs"
```

---

## Task 12: Update lib.rs exports and documentation

**Files:**
- Modify: `lib/runtime-settings/src/lib.rs`

**Step 1: Re-export VaultClient**

```rust
// Add to lib.rs exports:
pub use vault_client::{VaultClient, VaultClientBuilder};
```

**Step 2: Run full workspace tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Commit**

```bash
git add lib/runtime-settings/src/lib.rs
git commit -m "feat(runtime-settings): re-export VaultClient"
```

---

## Summary

After completing all tasks:

1. New `vault-client` library with:
   - Auto-detection of auth method (Token → K8s → OIDC)
   - Background token renewal
   - OIDC disk cache
   - Full KV v2 support

2. Updated `runtime-settings`:
   - Uses `vault-client` instead of `vaultrs`
   - `VaultClient` passed via builder
   - Re-exports `VaultClient` for convenience

3. Usage:
   ```rust
   use runtime_settings::{RuntimeSettings, VaultClient, setup, settings};

   let vault = VaultClient::from_env().await?;

   setup(
       RuntimeSettings::builder()
           .application("my-service")
           .vault_client(vault)
   ).await?;

   let value: Option<Arc<String>> = settings().get("KEY");
   ```
