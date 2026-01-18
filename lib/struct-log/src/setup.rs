use std::env;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::error::SetupError;
use crate::formatting_layer::JsonLogLayer;
use crate::storage::StorageLayer;

pub fn setup_logger(
    application_name: String,
    version: String,
) -> Result<Option<WorkerGuard>, SetupError> {
    if !env::var("JSON_LOG").is_ok_and(|s| s.parse().unwrap_or_default()) {
        tracing_subscriber::fmt::init();
        return Ok(None);
    }

    LogTracer::init().map_err(|_| SetupError::LogTracerAlreadyInitialized)?;

    let (non_blocking_writer, guard) = tracing_appender::non_blocking(std::io::stdout());

    let formatting_layer = JsonLogLayer::new(application_name, version, non_blocking_writer);
    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env())
        .with(StorageLayer)
        .with(formatting_layer);

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| SetupError::SubscriberAlreadySet)?;

    Ok(Some(guard))
}
