use std::collections::HashMap;

use async_trait::async_trait;

use crate::filters::SettingsService;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[async_trait]
pub trait RuntimeSettingsProvider {
    async fn get_settings(&self) -> Result<HashMap<String, Vec<Box<SettingsService>>>>;
}

mod microservice;

pub use microservice::MicroserviceRuntimeSettingsProvider;
