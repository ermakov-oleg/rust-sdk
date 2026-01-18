use super::{AuthMethod, OidcCache, TokenInfo};
use crate::VaultError;
use async_trait::async_trait;
use serde::Deserialize;
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
        let url = format!("{}/v1/auth/{}/oidc/auth_url", base_url, self.auth_method);

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
