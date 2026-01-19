// lib/runtime-settings/src/setup.rs
//! Global setup functions for RuntimeSettings.

use crate::error::SettingsError;
use crate::settings::{RuntimeSettings, RuntimeSettingsBuilder};
use std::sync::OnceLock;
use tokio::time::sleep;

static SETTINGS: OnceLock<RuntimeSettings> = OnceLock::new();

/// Get the global settings instance
pub fn settings() -> &'static RuntimeSettings {
    SETTINGS
        .get()
        .expect("RuntimeSettings not initialized - call setup() first")
}

/// Initialize global settings with builder
pub async fn setup(builder: RuntimeSettingsBuilder) -> Result<(), SettingsError> {
    let runtime_settings = builder.build()?;
    let refresh_interval = runtime_settings.refresh_interval;
    runtime_settings.init().await?;

    SETTINGS
        .set(runtime_settings)
        .map_err(|_| SettingsError::Vault("Settings already initialized".to_string()))?;

    // Start background refresh
    tokio::spawn(async move {
        loop {
            sleep(refresh_interval).await;
            if let Err(e) = settings().refresh().await {
                tracing::error!("Settings refresh failed: {}", e);
            }
        }
    });

    Ok(())
}

/// Initialize with default builder (requires RUNTIME_SETTINGS_APPLICATION env var)
pub async fn setup_from_env() -> Result<(), SettingsError> {
    let application =
        std::env::var("RUNTIME_SETTINGS_APPLICATION").unwrap_or_else(|_| "unknown".to_string());

    setup(RuntimeSettings::builder().application(application)).await
}
