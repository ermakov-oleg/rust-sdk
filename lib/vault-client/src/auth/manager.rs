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
