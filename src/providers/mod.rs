pub use file::FileProvider;
pub use microservice::{DiffSettings, MicroserviceRuntimeSettingsProvider};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod file;
mod microservice;

