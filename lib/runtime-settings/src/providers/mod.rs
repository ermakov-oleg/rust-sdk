// lib/runtime-settings/src/providers/mod.rs
pub mod env;
pub mod file;
pub mod mcs;

use crate::entities::{RawSetting, SettingKey};
use crate::error::SettingsError;
use async_trait::async_trait;

/// Response from a settings provider
#[derive(Debug, Clone, Default)]
pub struct ProviderResponse {
    pub settings: Vec<RawSetting>,
    pub deleted: Vec<SettingKey>,
    pub version: String,
}

/// Trait for settings providers
#[async_trait]
pub trait SettingsProvider: Send + Sync {
    /// Load settings. Returns settings, deleted keys, and new version.
    async fn load(&self, current_version: &str) -> Result<ProviderResponse, SettingsError>;

    /// Default priority for settings from this provider
    fn default_priority(&self) -> i64;

    /// Provider name for logging
    fn name(&self) -> &'static str;
}

pub use env::EnvProvider;
pub use file::FileProvider;
pub use mcs::McsProvider;
