// lib/runtime-settings/src/providers/file.rs
use super::{ProviderResponse, SettingsProvider};
use crate::entities::RawSetting;
use crate::error::SettingsError;
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

const FILE_DEFAULT_PRIORITY: i64 = 1_000_000_000_000_000_000;

/// Setting as stored in file (priority is optional)
#[derive(Debug, Deserialize)]
struct FileSetting {
    key: String,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    filter: HashMap<String, String>,
    value: serde_json::Value,
}

pub struct FileProvider {
    path: PathBuf,
}

impl FileProvider {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create from RUNTIME_SETTINGS_FILE_PATH env var or default
    pub fn from_env() -> Self {
        let path = std::env::var("RUNTIME_SETTINGS_FILE_PATH")
            .unwrap_or_else(|_| "runtime-settings.json".to_string());
        Self::new(PathBuf::from(path))
    }
}

#[async_trait]
impl SettingsProvider for FileProvider {
    async fn load(&self, _current_version: &str) -> Result<ProviderResponse, SettingsError> {
        let content = tokio::fs::read_to_string(&self.path).await?;

        // Parse as JSON5 to support comments
        let file_settings: Vec<FileSetting> = json5::from_str(&content).map_err(|e| {
            SettingsError::JsonParse(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )))
        })?;

        let settings = file_settings
            .into_iter()
            .map(|fs| RawSetting {
                key: fs.key,
                priority: fs.priority.unwrap_or(FILE_DEFAULT_PRIORITY),
                filter: fs.filter,
                value: fs.value,
            })
            .collect();

        Ok(ProviderResponse {
            settings,
            deleted: vec![],
            version: String::new(),
        })
    }

    fn default_priority(&self) -> i64 {
        FILE_DEFAULT_PRIORITY
    }

    fn name(&self) -> &'static str {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_file_provider_loads_json() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"[{{"key": "TEST_KEY", "priority": 100, "value": "test"}}]"#
        )
        .unwrap();

        let provider = FileProvider::new(file.path().to_path_buf());
        let response = provider.load("").await.unwrap();

        assert_eq!(response.settings.len(), 1);
        assert_eq!(response.settings[0].key, "TEST_KEY");
        assert_eq!(response.settings[0].priority, 100);
    }

    #[tokio::test]
    async fn test_file_provider_default_priority() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"[{{"key": "KEY", "value": 123}}]"#).unwrap();

        let provider = FileProvider::new(file.path().to_path_buf());
        let response = provider.load("").await.unwrap();

        assert_eq!(response.settings[0].priority, 1_000_000_000_000_000_000);
    }

    #[tokio::test]
    async fn test_file_provider_missing_file() {
        let provider = FileProvider::new("/nonexistent/path.json".into());
        let result = provider.load("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_provider_json5_comments() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"[
            // This is a comment
            {{"key": "KEY", "value": "val"}}
        ]"#
        )
        .unwrap();

        let provider = FileProvider::new(file.path().to_path_buf());
        let response = provider.load("").await.unwrap();

        assert_eq!(response.settings.len(), 1);
    }
}
