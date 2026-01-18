use std::env;
use std::io;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::error::SetupError;
use crate::formatting_layer::JsonLogLayer;
use crate::storage::StorageLayer;

/// Builder for configuring structured JSON logging
pub struct StructLogBuilder {
    application: String,
    version: String,
    hostname: Option<String>,
    json_enabled: bool,
}

impl StructLogBuilder {
    /// Create a new builder with required application name and version
    pub fn new(application: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            application: application.into(),
            version: version.into(),
            hostname: None,
            json_enabled: true,
        }
    }

    /// Set a custom hostname (defaults to system hostname)
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Enable or disable JSON output (defaults to true)
    pub fn json_enabled(mut self, enabled: bool) -> Self {
        self.json_enabled = enabled;
        self
    }

    /// Read JSON_LOG env var to determine if JSON should be enabled
    pub fn json_from_env(mut self) -> Self {
        self.json_enabled = env::var("JSON_LOG").is_ok_and(|s| s.parse().unwrap_or_default());
        self
    }

    /// Initialize the logger with the configured settings
    pub fn init(self) -> Result<Option<WorkerGuard>, SetupError> {
        if !self.json_enabled {
            tracing_subscriber::fmt::init();
            return Ok(None);
        }

        LogTracer::init().map_err(|_| SetupError::LogTracerAlreadyInitialized)?;

        let (non_blocking, guard) = tracing_appender::non_blocking(io::stdout());

        let layer = match self.hostname {
            Some(hostname) => {
                JsonLogLayer::with_hostname(self.application, self.version, hostname, non_blocking)
            }
            None => JsonLogLayer::new(self.application, self.version, non_blocking),
        };

        let subscriber = Registry::default()
            .with(EnvFilter::from_default_env())
            .with(StorageLayer)
            .with(layer);

        tracing::subscriber::set_global_default(subscriber)
            .map_err(|_| SetupError::SubscriberAlreadySet)?;

        Ok(Some(guard))
    }
}
