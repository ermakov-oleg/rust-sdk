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
            .map_err(|e| {
                VaultError::KubernetesError(format!(
                    "Failed to read JWT from {}: {}",
                    self.jwt_path, e
                ))
            })
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
    use std::io::Write;
    use tempfile::NamedTempFile;

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
