use std::collections::HashMap;

use async_trait::async_trait;

use crate::filters::SettingsService;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod file;
mod microservice;

pub use microservice::{MicroserviceRuntimeSettingsProvider, DiffSettings};
// pub use file::get_settings;
