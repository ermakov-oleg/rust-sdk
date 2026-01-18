// lib/runtime-settings/src/providers/mcs.rs

use super::{ProviderResponse, SettingsProvider};
use crate::entities::McsResponse;
use crate::error::SettingsError;
use async_trait::async_trait;
use serde::Serialize;
use uuid::Uuid;

const DEFAULT_MCS_BASE_URL: &str = "http://localhost:8080";

#[derive(Debug, Serialize)]
struct McsRequest {
    runtime: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    application: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mcs_run_env: Option<String>,
}

pub struct McsProvider {
    base_url: String,
    application: String,
    mcs_run_env: Option<String>,
    client: reqwest::Client,
}

impl McsProvider {
    pub fn new(base_url: String, application: String, mcs_run_env: Option<String>) -> Self {
        Self {
            base_url,
            application,
            mcs_run_env,
            client: reqwest::Client::new(),
        }
    }

    /// Create from environment variables
    pub fn from_env(application: String) -> Self {
        let base_url = std::env::var("RUNTIME_SETTINGS_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_MCS_BASE_URL.to_string());
        let mcs_run_env = std::env::var("MCS_RUN_ENV").ok();
        Self::new(base_url, application, mcs_run_env)
    }
}

#[async_trait]
impl SettingsProvider for McsProvider {
    async fn load(&self, current_version: &str) -> Result<ProviderResponse, SettingsError> {
        let url = format!("{}/v3/get-runtime-settings/", self.base_url);

        let request = McsRequest {
            runtime: "rust".to_string(),
            version: current_version.to_string(),
            application: Some(self.application.clone()),
            mcs_run_env: self.mcs_run_env.clone(),
        };

        let operation_id = Uuid::new_v4().to_string();
        let response = self
            .client
            .get(&url)
            .query(&request)
            .header("X-OperationId", &operation_id)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SettingsError::McsResponse {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let mcs_response: McsResponse = response.json().await?;

        Ok(ProviderResponse {
            settings: mcs_response.settings,
            deleted: mcs_response.deleted,
            version: mcs_response.version,
        })
    }

    fn default_priority(&self) -> i64 {
        0 // MCS settings have their own priority
    }

    fn name(&self) -> &'static str {
        "mcs"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcs_request_serialization() {
        let req = McsRequest {
            runtime: "rust".to_string(),
            version: "42".to_string(),
            application: Some("my-app".to_string()),
            mcs_run_env: Some("PROD".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""runtime":"rust""#));
        assert!(json.contains(r#""version":"42""#));
    }

    #[test]
    fn test_mcs_request_skips_none_fields() {
        let req = McsRequest {
            runtime: "rust".to_string(),
            version: "1".to_string(),
            application: None,
            mcs_run_env: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("application"));
        assert!(!json.contains("mcs_run_env"));
    }

    #[test]
    fn test_mcs_provider_name() {
        let provider = McsProvider::new("http://test.local".to_string(), "app".to_string(), None);
        assert_eq!(provider.name(), "mcs");
    }

    #[test]
    fn test_mcs_provider_default_priority() {
        let provider = McsProvider::new(
            "http://test.local".to_string(),
            "app".to_string(),
            Some("DEV".to_string()),
        );
        assert_eq!(provider.default_priority(), 0);
    }
}
