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

        let auth_method: Arc<dyn AuthMethod> = if let Some(token) = config.token {
            Arc::new(StaticTokenAuth::new(token))
        } else if config.is_kubernetes {
            Arc::new(
                KubernetesAuth::new(config.k8s_auth_method, config.role)
                    .with_jwt_path(config.k8s_jwt_path),
            )
        } else {
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
    pub async fn from_env() -> Result<Self, VaultError> {
        VaultClientBuilder::new().build().await
    }

    pub fn builder() -> VaultClientBuilder {
        VaultClientBuilder::new()
    }

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
