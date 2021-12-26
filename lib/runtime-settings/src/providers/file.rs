use crate::entities::Setting;
use crate::providers::{Result, RuntimeSettingsState, SettingsProvider};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs::read_to_string;

pub struct FileProvider {
    path: PathBuf,
}

impl FileProvider {
    pub fn new(path: String) -> FileProvider {
        let mut absolute_path = std::env::current_dir().unwrap();
        absolute_path.push(&path);
        FileProvider {
            path: absolute_path,
        }
    }

    async fn read_settings(&self) -> Result<Vec<Setting>> {
        let contents = read_to_string(&self.path).await?;
        let result: Vec<Setting> = serde_json::from_str(contents.as_str())?;
        Ok(result)
    }
}

#[async_trait]
impl SettingsProvider for FileProvider {
    async fn update_settings(&self, state: &dyn RuntimeSettingsState) {
        tracing::debug!("Load settings from file {:?} ...", &self.path);
        match self.read_settings().await {
            Ok(settings) => state.update_settings(settings, vec![]),
            Err(err) => {
                tracing::error!(
                    error = ?err,
                    "Error: Could not update settings from file: {:?}",
                    &self.path,
                )
            }
        };
    }
}
