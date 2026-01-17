// lib/runtime-settings/src/settings.rs
//! RuntimeSettings - main struct for managing runtime configuration.

use crate::context::{Context, Request, StaticContext};
use crate::entities::Setting;
use crate::error::SettingsError;
use crate::filters::{check_static_filters};
use crate::providers::{
    EnvProvider, FileProvider, McsProvider, ProviderResponse, SettingsProvider,
};
use crate::scoped::{current_context, current_request, set_thread_context, set_thread_request};
use crate::scoped::{with_task_context, with_task_request, ContextGuard, RequestGuard};
use crate::secrets::SecretsService;
use crate::watchers::{Watcher, WatcherId, WatchersService};
use semver::Version;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

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
                    self.merge_settings(response).await;
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
                    let state = self.state.read().await;
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
                        self.merge_settings(response).await;
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
        let current_values = self.collect_current_values().await;
        self.watchers.check(&current_values).await;

        Ok(())
    }

    /// Get setting value, panics if context not set
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let ctx = self
            .get_effective_context()
            .expect("Context not set - call set_context() or with_context() first");
        self.get_internal(key, &ctx)
    }

    /// Get setting value with default
    pub fn get_or<T: DeserializeOwned + Clone>(&self, key: &str, default: T) -> T {
        self.get::<T>(key).unwrap_or(default)
    }

    /// Create a getter function for a setting
    pub fn getter<T: DeserializeOwned + Clone + 'static>(
        &self,
        key: &'static str,
        default: T,
    ) -> impl Fn(&RuntimeSettings) -> T {
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

    /// Set thread-local context
    pub fn set_context(&self, ctx: Context) -> ContextGuard {
        set_thread_context(ctx)
    }

    /// Set thread-local request
    pub fn set_request(&self, req: Request) -> RequestGuard {
        set_thread_request(req)
    }

    /// Execute async closure with task-local context
    pub async fn with_context<F, T>(&self, ctx: Context, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        with_task_context(ctx, f).await
    }

    /// Execute async closure with task-local request
    pub async fn with_request<F, T>(&self, req: Request, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        with_task_request(req, f).await
    }

    /// Get effective context by merging scoped context with static
    fn get_effective_context(&self) -> Option<Context> {
        // Try to get scoped context first
        let scoped_ctx = current_context();
        let scoped_req = current_request();

        // If we have a scoped context, use it
        if let Some(ctx) = scoped_ctx {
            return Some(ctx);
        }

        // If we have a scoped request but no context, create context from static + request
        if let Some(req) = scoped_req {
            return Some(Context {
                application: self.static_context.application.clone(),
                server: self.static_context.server.clone(),
                environment: self.static_context.environment.clone(),
                libraries_versions: self.static_context.libraries_versions.clone(),
                mcs_run_env: self.static_context.mcs_run_env.clone(),
                request: Some(req),
                custom: HashMap::new(),
            });
        }

        // No scoped context at all
        None
    }

    /// Internal get with explicit context
    fn get_internal<T: DeserializeOwned>(&self, key: &str, ctx: &Context) -> Option<T> {
        // Use block_on to read from async RwLock
        let state = futures::executor::block_on(self.state.read());

        let settings = state.settings.get(key)?;

        // Find the first matching setting (they're sorted by priority)
        for setting in settings {
            // Check dynamic filters using compiled filters
            if setting.check_dynamic_filters(ctx) {
                // Deserialize the value
                match serde_json::from_value(setting.value.clone()) {
                    Ok(v) => return Some(v),
                    Err(e) => {
                        tracing::warn!(
                            key = key,
                            error = %e,
                            "Failed to deserialize setting value"
                        );
                        return None;
                    }
                }
            }
        }

        None
    }

    /// Merge provider response into state
    async fn merge_settings(&self, response: ProviderResponse) {
        let mut state = self.state.write().await;

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
    async fn collect_current_values(&self) -> HashMap<String, serde_json::Value> {
        let state = self.state.read().await;
        let mut values = HashMap::new();

        // Get effective context
        let ctx = match self.get_effective_context() {
            Some(c) => c,
            None => {
                // Create minimal context from static
                Context {
                    application: self.static_context.application.clone(),
                    server: self.static_context.server.clone(),
                    environment: self.static_context.environment.clone(),
                    libraries_versions: self.static_context.libraries_versions.clone(),
                    mcs_run_env: self.static_context.mcs_run_env.clone(),
                    request: None,
                    custom: HashMap::new(),
                }
            }
        };

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
                    .unwrap_or_else(|| "http://master.runtime-settings.dev3.cian.ru".to_string())
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

        // Try to create secrets service from env, fallback to no vault
        let secrets =
            SecretsService::from_env().unwrap_or_else(|_| SecretsService::new_without_vault());

        RuntimeSettings {
            providers,
            state: RwLock::new(SettingsState::default()),
            secrets,
            watchers: WatchersService::new(),
            static_context,
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
    #[should_panic(expected = "Context not set")]
    fn test_get_requires_context() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        // This should panic because no context is set
        let _: Option<String> = settings.get("SOME_KEY");
    }

    #[test]
    fn test_get_with_context() {
        let settings = RuntimeSettings::builder()
            .application("test-app")
            .mcs_enabled(false)
            .env_enabled(false)
            .build();

        let ctx = Context {
            application: "test-app".to_string(),
            ..Default::default()
        };

        let _guard = settings.set_context(ctx);

        // Should not panic, but return None since no settings loaded
        let result: Option<String> = settings.get("SOME_KEY");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_merge_settings_empty_settings() {
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

        settings.merge_settings(response).await;

        let state = settings.state.read().await;
        assert_eq!(state.version, "1");
        assert!(state.settings.contains_key("MY_KEY"));
        assert_eq!(state.settings["MY_KEY"].len(), 1);
        assert_eq!(
            state.settings["MY_KEY"][0].value,
            serde_json::json!("test_value")
        );
    }

    #[tokio::test]
    async fn test_merge_settings_priority_order() {
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
        settings.merge_settings(response1).await;

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
        settings.merge_settings(response2).await;

        let state = settings.state.read().await;
        assert_eq!(state.settings["MY_KEY"].len(), 2);
        // Highest priority should be first
        assert_eq!(state.settings["MY_KEY"][0].priority, 100);
        assert_eq!(state.settings["MY_KEY"][1].priority, 10);
    }

    #[tokio::test]
    async fn test_get_internal_returns_highest_priority() {
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
        settings.merge_settings(response).await;

        let ctx = Context {
            application: "test-app".to_string(),
            ..Default::default()
        };

        let result: Option<String> = settings.get_internal("MY_KEY", &ctx);
        assert_eq!(result, Some("high_priority".to_string()));
    }

    #[tokio::test]
    async fn test_with_context_async() {
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
        settings.merge_settings(response).await;

        let ctx = Context {
            application: "test-app".to_string(),
            ..Default::default()
        };

        let result = settings
            .with_context(ctx, async {
                let value: Option<String> = settings.get("TEST_KEY");
                value
            })
            .await;

        assert_eq!(result, Some("async_value".to_string()));
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

        let ctx = Context {
            application: "test-app".to_string(),
            ..Default::default()
        };

        let _guard = settings.set_context(ctx);

        // Should return default since key doesn't exist
        let result: String = settings.get_or("NONEXISTENT_KEY", "default_value".to_string());
        assert_eq!(result, "default_value");
    }
}
