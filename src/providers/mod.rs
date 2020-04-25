use std::collections::HashMap;

use async_trait::async_trait;

use crate::filters::SettingsService;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[async_trait]
pub trait RuntimeSettingsProvider {
    async fn get_settings(&mut self) -> Result<HashMap<String, Vec<SettingsService>>>;
}

mod microservice;

pub use microservice::MicroserviceRuntimeSettingsProvider;
