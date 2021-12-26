use crate::entities::{Setting, SettingKey};
use async_trait::async_trait;
pub use file::FileProvider;
pub use microservice::MicroserviceRuntimeSettingsProvider;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod file;
mod microservice;

pub trait RuntimeSettingsState: Send + Sync {
    fn get_version(&self) -> String;
    fn set_version(&self, version: String);
    fn update_settings(&self, new_settings: Vec<Setting>, to_delete: Vec<SettingKey>);
}

#[async_trait]
pub trait SettingsProvider: Send + Sync {
    async fn update_settings(&self, state: &dyn RuntimeSettingsState);
}
