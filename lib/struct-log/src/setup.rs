use crate::formatting_layer::JsonLogLayer;
use std::env;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_bunyan_formatter::JsonStorageLayer;
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn setup_logger(application_name: String, version: String) -> Option<WorkerGuard> {
    if !env::var("JSON_LOG").map_or(false, |s| s.parse().unwrap_or_default()) {
        tracing_subscriber::fmt::init();
        return None;
    }

    // Redirect the logs from log library to tracing's subscribers.
    LogTracer::init().expect("Unable to setup log tracer!");

    // Non-blocking stdout writer
    let (non_blocking_writer, guard) = tracing_appender::non_blocking(std::io::stdout());

    let bunyan_formatting_layer = JsonLogLayer::new(application_name, version, non_blocking_writer);
    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env())
        .with(JsonStorageLayer)
        .with(bunyan_formatting_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();
    Some(guard)
}
