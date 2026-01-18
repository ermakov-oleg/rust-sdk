// lib/runtime-settings/src/settings.rs
//! RuntimeSettings - main struct for managing runtime configuration.

use crate::context::{DynamicContext, Request, StaticContext};
use crate::entities::Setting;
use crate::error::SettingsError;
use crate::filters::check_static_filters;
use crate::providers::{
    EnvProvider, FileProvider, McsProvider, ProviderResponse, SettingsProvider,
};
use crate::scoped::{
    current_custom, current_request, set_thread_custom, set_thread_request, with_task_custom,
    with_task_request, CustomContextGuard, RequestGuard,
};
use crate::secrets::SecretsService;
use crate::watchers::{Watcher, WatcherId, WatchersService};
use semver::Version;
use vault_client::VaultClient;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Internal state of RuntimeSettings
struct SettingsState {
    version: String,
    settings: HashMap<String, Vec<Setting>>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            version: "0".to_string(),
            settings: HashMap::new(),
        }
    }
}

/// Main runtime settings manager
pub struct RuntimeSettings {
    providers: Vec<Box<dyn SettingsProvider>>,
    state: RwLock<SettingsState>,
    secrets: SecretsService,
    watchers: WatchersService,
    pub(crate) static_context: StaticContext,
    pub(crate) refresh_interval: Duration,
}

impl RuntimeSettings {
    /// Create a new builder for RuntimeSettings
    pub fn builder() -> RuntimeSettingsBuilder {
        RuntimeSettingsBuilder::new()
    }

    /// Initialize settings by loading from all providers
    pub async fn init(&self) -> Result<(), SettingsError> {
        for provider in &self.providers {
            match provider.load("").await {
                Ok(response) => {
                    tracing::info!(
                        provider = provider.name(),
                        settings_count = response.settings.len(),
                        "Loaded settings from provider"
                    );
                    self.merge_settings(response);
                }
                Err(e) => {
                    tracing::warn!(
                        provider = provider.name(),
                        error = %e,
                        "Failed to load settings from provider"
                    );
                    // Continue with other providers
                }
            }
        }
        Ok(())
    }

    /// Refresh settings from MCS and secrets, then check watchers
    pub async fn refresh(&self) -> Result<(), SettingsError> {
        // Find MCS provider and refresh
        for provider in &self.providers {
            if provider.name() == "mcs" {
                let version = {
                    let state = self.state.read().unwrap();
                    state.version.clone()
                };

                match provider.load(&version).await {
                    Ok(response) => {
                        tracing::debug!(
                            settings_count = response.settings.len(),
                            deleted_count = response.deleted.len(),
                            new_version = %response.version,
                            "Refreshed settings from MCS"
                        );
                        self.merge_settings(response);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to refresh settings from MCS");
                    }
                }
            }
        }

        // Refresh secrets
        self.secrets.refresh().await?;

        // Check watchers
        let current_values = self.collect_current_values();
        self.watchers.check(&current_values).await;

        Ok(())
    }

    /// Refresh settings with a configurable timeout
    pub async fn refresh_with_timeout(&self, timeout: Duration) -> Result<(), SettingsError> {
        tokio::time::timeout(timeout, self.refresh())
            .await
            .map_err(|_| SettingsError::Timeout)?
    }

    /// Get setting value using current scoped context
    pub fn get<T>(&self, key: &str) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let ctx = self.get_dynamic_context();
        self.get_internal(key, &ctx)
    }

    /// Get setting value with default
    pub fn get_or<T>(&self, key: &str, default: T) -> Arc<T>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        self.get(key).unwrap_or_else(|| Arc::new(default))
    }

    /// Create a getter function for a setting
    pub fn getter<T>(&self, key: &'static str, default: T) -> impl Fn(&RuntimeSettings) -> Arc<T>
    where
        T: DeserializeOwned + Send + Sync + Clone + 'static,
    {
        move |settings| settings.get_or::<T>(key, default.clone())
    }

    /// Add a watcher for a setting
    pub fn add_watcher(&self, key: &str, watcher: Watcher) -> WatcherId {
        self.watchers.add(key, watcher)
    }

    /// Remove a watcher by ID
    pub fn remove_watcher(&self, id: WatcherId) {
        self.watchers.remove(id)
    }

    /// Set thread-local request
    pub fn set_request(&self, req: Request) -> RequestGuard {
        set_thread_request(req)
    }

    /// Set thread-local custom context layer
    pub fn set_custom(&self, values: HashMap<String, String>) -> CustomContextGuard {
        set_thread_custom(values)
    }

    /// Execute async closure with task-local request
    pub async fn with_request<F, T>(&self, req: Request, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        with_task_request(req, f).await
    }

    /// Execute async closure with additional custom context layer
    pub async fn with_custom<F, T>(&self, values: HashMap<String, String>, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        with_task_custom(values, f).await
    }

    /// Get dynamic context from scoped request and custom
    fn get_dynamic_context(&self) -> DynamicContext {
        DynamicContext {
            request: current_request(),
            custom: current_custom(),
        }
    }

    /// Internal get with explicit context
    fn get_internal<T>(&self, key: &str, ctx: &DynamicContext) -> Option<Arc<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let state = self.state.read().unwrap();

        let settings = state.settings.get(key)?;

        // Find the first matching setting (they're sorted by priority)
        for setting in settings {
            // Check dynamic filters using compiled filters
            if setting.check_dynamic_filters(ctx) {
                // Invalidate cache if secrets version changed
                if setting.has_secrets() {
                    setting.invalidate_if_stale(self.secrets.version());
                }

                return setting.get_value::<T>(&self.secrets);
            }
        }

        None
    }

    /// Merge provider response into state
    fn merge_settings(&self, response: ProviderResponse) {
        let mut state = self.state.write().unwrap();

        // Process deleted settings first
        for deleted in &response.deleted {
            if let Some(settings) = state.settings.get_mut(&deleted.key) {
                settings.retain(|s| s.priority != deleted.priority);
            }
        }

        // Process new/updated settings
        for raw_setting in response.settings {
            // Check static filters before compiling
            if !check_static_filters(&raw_setting.filter, &self.static_context) {
                // Setting doesn't match static filters, remove if exists
                if let Some(settings) = state.settings.get_mut(&raw_setting.key) {
                    settings.retain(|s| s.priority != raw_setting.priority);
                }
                continue;
            }

            // Compile the raw setting
            let setting = match Setting::compile(raw_setting) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to compile setting filters, skipping");
                    continue;
                }
            };

            // Add or update setting
            let settings = state.settings.entry(setting.key.clone()).or_default();

            // Remove existing setting with same priority
            settings.retain(|s| s.priority != setting.priority);

            // Insert in priority order (highest first)
            let pos = settings
                .iter()
                .position(|s| s.priority < setting.priority)
                .unwrap_or(settings.len());
            settings.insert(pos, setting);
        }

        // Update version if non-empty
        if !response.version.is_empty() {
            state.version = response.version;
        }
    }

    /// Collect current values for watched settings
    fn collect_current_values(&self) -> HashMap<String, serde_json::Value> {
        let state = self.state.read().unwrap();
        let mut values = HashMap::new();

        let ctx = self.get_dynamic_context();
        for (key, settings) in &state.settings {
            for setting in settings {
                if setting.check_dynamic_filters(&ctx) {
                    values.insert(key.clone(), setting.value.clone());
                    break;
                }
            }
        }

        values
    }
}

/// Builder for RuntimeSettings
pub struct RuntimeSettingsBuilder {
    application: String,
    server: String,
    environment: HashMap<String, String>,
    libraries_versions: HashMap<String, Version>,
    mcs_run_env: Option<String>,
    mcs_enabled: bool,
    mcs_base_url: Option<String>,
    file_path: Option<String>,
    env_enabled: bool,
    refresh_interval: Duration,
    vault_client: Option<VaultClient>,
}

impl RuntimeSettingsBuilder {
    /// Create a new builder with defaults
    pub fn new() -> Self {
        let server = gethostname::gethostname().to_string_lossy().to_string();
        let environment: HashMap<String, String> = std::env::vars().collect();
        let mcs_run_env = environment.get("MCS_RUN_ENV").cloned();

        Self {
            application: String::new(),
            server,
            environment,
            libraries_versions: HashMap::new(),
            mcs_run_env,
            mcs_enabled: true,
            mcs_base_url: None,
            file_path: None,
            env_enabled: true,
            refresh_interval: Duration::from_secs(30),
            vault_client: None,
        }
    }

    /// Set application name
    pub fn application(mut self, name: impl Into<String>) -> Self {
        self.application = name.into();
        self
    }

    /// Set server name
    pub fn server(mut self, name: impl Into<String>) -> Self {
        self.server = name.into();
        self
    }

    /// Add a library version
    pub fn library_version(mut self, name: impl Into<String>, version: Version) -> Self {
        self.libraries_versions.insert(name.into(), version);
        self
    }

    /// Enable or disable MCS provider
    pub fn mcs_enabled(mut self, enabled: bool) -> Self {
        self.mcs_enabled = enabled;
        self
    }

    /// Set MCS base URL
    pub fn mcs_base_url(mut self, url: impl Into<String>) -> Self {
        self.mcs_base_url = Some(url.into());
        self
    }

    /// Set file path for file provider
    pub fn file_path(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Enable or disable env provider
    pub fn env_enabled(mut self, enabled: bool) -> Self {
        self.env_enabled = enabled;
        self
    }

    /// Set the refresh interval for background settings updates
    pub fn refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh_interval = interval;
        self
    }

    /// Set VaultClient for secrets
    pub fn vault_client(mut self, client: VaultClient) -> Self {
        self.vault_client = Some(client);
        self
    }

    /// Build the RuntimeSettings instance
    pub fn build(self) -> RuntimeSettings {
        let mut providers: Vec<Box<dyn SettingsProvider>> = Vec::new();

        // Add env provider first (lowest priority)
        if self.env_enabled {
            providers.push(Box::new(EnvProvider::new(self.environment.clone())));
        }

        // Add file provider
        if let Some(path) = &self.file_path {
            providers.push(Box::new(FileProvider::new(PathBuf::from(path))));
        }

        // Add MCS provider last (to get the latest settings)
        if self.mcs_enabled {
            let base_url = self.mcs_base_url.unwrap_or_else(|| {
                self.environment
                    .get("RUNTIME_SETTINGS_BASE_URL")
                    .cloned()
                    .unwrap_or_else(|| "http://localhost:8080".to_string())
            });
            providers.push(Box::new(McsProvider::new(
                base_url,
                self.application.clone(),
                self.mcs_run_env.clone(),
            )));
        }

        let static_context = StaticContext {
            application: self.application,
            server: self.server,
            environment: self.environment,
            libraries_versions: self.libraries_versions,
            mcs_run_env: self.mcs_run_env,
        };

        // Create secrets service with vault client if provided
        let secrets = match self.vault_client {
            Some(client) => SecretsService::new(client),
            None => SecretsService::new_without_vault(),
        };

        RuntimeSettings {
            providers,
            state: RwLock::new(SettingsState::default()),
            secrets,
            watchers: WatchersService::new(),
            static_context,
            refresh_interval: self.refresh_interval,
        }
    }
}

impl Default for RuntimeSettingsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::RawSetting;
    use std::sync::Arc;

    #[test]
    fn test_builder_basic() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .server("test-server")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        assert_eq!(settings.static_context.application, "test-app");
        assert_eq!(settings.static_context.server, "test-server");
    }

    #[test]
    fn test_builder_with_library_version() {
        let version = Version::parse("1.2.3").unwrap();
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .library_version("my-lib", version.clone())
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        assert_eq!(
            settings.static_context.libraries_versions.get("my-lib"),
            Some(&version)
        );
    }

    #[test]
    fn test_get_without_context() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        // Should not panic, just return None since no settings loaded
        let result: Option<Arc<String>> = settings.get("SOME_KEY");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_with_request() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        let req = Request {
            method: "GET".to_string(),
            path: "/api".to_string(),
            headers: HashMap::new(),
        };

        let _guard = settings.set_request(req);

        // Should not panic, but return None since no settings loaded
        let result: Option<Arc<String>> = settings.get("SOME_KEY");
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_settings_empty_settings() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        let response = ProviderResponse {
            settings: vec![RawSetting {
                key: "MY_KEY".to_string(),
                priority: 100,
                filter: HashMap::new(),
                value: serde_json::json!("test_value"),
            }],
            deleted: vec![],
            version: "1".to_string(),
        };

        settings.merge_settings(response);

        let state = settings.state.read().unwrap();
        assert_eq!(state.version, "1");
        assert!(state.settings.contains_key("MY_KEY"));
        assert_eq!(state.settings["MY_KEY"].len(), 1);
        assert_eq!(
            state.settings["MY_KEY"][0].value,
            serde_json::json!("test_value")
        );
    }

    #[test]
    fn test_merge_settings_priority_order() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        // Add low priority setting
        let response1 = ProviderResponse {
            settings: vec![RawSetting {
                key: "MY_KEY".to_string(),
                priority: 10,
                filter: HashMap::new(),
                value: serde_json::json!("low_priority"),
            }],
            deleted: vec![],
            version: "1".to_string(),
        };
        settings.merge_settings(response1);

        // Add high priority setting
        let response2 = ProviderResponse {
            settings: vec![RawSetting {
                key: "MY_KEY".to_string(),
                priority: 100,
                filter: HashMap::new(),
                value: serde_json::json!("high_priority"),
            }],
            deleted: vec![],
            version: "2".to_string(),
        };
        settings.merge_settings(response2);

        let state = settings.state.read().unwrap();
        assert_eq!(state.settings["MY_KEY"].len(), 2);
        // Highest priority should be first
        assert_eq!(state.settings["MY_KEY"][0].priority, 100);
        assert_eq!(state.settings["MY_KEY"][1].priority, 10);
    }

    #[test]
    fn test_get_internal_returns_highest_priority() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        // Add settings with different priorities
        let response = ProviderResponse {
            settings: vec![
                RawSetting {
                    key: "MY_KEY".to_string(),
                    priority: 100,
                    filter: HashMap::new(),
                    value: serde_json::json!("high_priority"),
                },
                RawSetting {
                    key: "MY_KEY".to_string(),
                    priority: 10,
                    filter: HashMap::new(),
                    value: serde_json::json!("low_priority"),
                },
            ],
            deleted: vec![],
            version: "1".to_string(),
        };
        settings.merge_settings(response);

        let ctx = DynamicContext::default();

        let result: Option<Arc<String>> = settings.get_internal("MY_KEY", &ctx);
        assert_eq!(result.as_deref(), Some(&"high_priority".to_string()));
    }

    #[tokio::test]
    async fn test_with_request_async() {
        let settings = Arc::new(
            RuntimeSettings::builder()
                .application("test-app")
                .mcs_enabled(false)
                .env_enabled(false)
                .build(),
        );

        // Add a setting
        let response = ProviderResponse {
            settings: vec![RawSetting {
                key: "TEST_KEY".to_string(),
                priority: 100,
                filter: HashMap::new(),
                value: serde_json::json!("async_value"),
            }],
            deleted: vec![],
            version: "1".to_string(),
        };
        settings.merge_settings(response);

        let req = Request {
            method: "GET".to_string(),
            path: "/api".to_string(),
            headers: HashMap::new(),
        };

        let result = settings
            .with_request(req, async {
                let value: Option<Arc<String>> = settings.get("TEST_KEY");
                value
            })
            .await;

        assert_eq!(result.as_deref(), Some(&"async_value".to_string()));
    }

    #[tokio::test]
    async fn test_with_custom_async() {
        let settings = Arc::new(
            RuntimeSettings::builder()
                .application("test-app")
                .mcs_enabled(false)
                .env_enabled(false)
                .build(),
        );

        // Add a setting
        let response = ProviderResponse {
            settings: vec![RawSetting {
                key: "TEST_KEY".to_string(),
                priority: 100,
                filter: HashMap::new(),
                value: serde_json::json!("async_value"),
            }],
            deleted: vec![],
            version: "1".to_string(),
        };
        settings.merge_settings(response);

        let custom: HashMap<String, String> = [("key".to_string(), "value".to_string())].into();

        let result = settings
            .with_custom(custom, async {
                let value: Option<Arc<String>> = settings.get("TEST_KEY");
                value
            })
            .await;

        assert_eq!(result.as_deref(), Some(&"async_value".to_string()));
    }

    #[test]
    fn test_builder_default() {
        let builder = RuntimeSettingsBuilder::default();
        assert!(builder.mcs_enabled);
        assert!(builder.env_enabled);
        assert!(builder.application.is_empty());
    }

    #[tokio::test]
    async fn test_init_loads_from_providers() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(true) // Enable env provider
            .build();

        // This should not fail even if file doesn't exist
        let result = settings.init().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_or_with_default() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        // Should return default since key doesn't exist (no context needed now)
        let result: Arc<String> = settings.get_or("NONEXISTENT_KEY", "default_value".to_string());
        assert_eq!(*result, "default_value");
    }

    #[test]
    fn test_set_custom() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        let custom: HashMap<String, String> =
            [("test_key".to_string(), "test_value".to_string())].into();

        {
            let _guard = settings.set_custom(custom);
            // Custom context should be set
            let ctx = settings.get_dynamic_context();
            assert_eq!(ctx.custom.get("test_key"), Some("test_value"));
        }

        // After guard is dropped, custom context should be empty
        let ctx = settings.get_dynamic_context();
        assert!(ctx.custom.is_empty());
    }

    #[tokio::test]
    async fn test_refresh_with_timeout_succeeds() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        // With no providers, refresh should complete instantly
        let result = settings.refresh_with_timeout(Duration::from_secs(1)).await;
        assert!(result.is_ok());
    }
}
