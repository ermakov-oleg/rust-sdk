use std::env;

use tracing_appender::non_blocking::WorkerGuard;

mod consts;
mod settings;
mod web;

#[tokio::main]
async fn main() -> Result<(), ()> {
    let _guard = init_logger();

    let runtime_settings = settings::setup().await;
    let key = "SOME_KEY";
    let val: Option<String> = runtime_settings.get(key, &settings::get_context());
    tracing::warn!("Settings {}:{:#?}", key, val);

    web::start_server().await;
    Ok(())
}

fn init_logger() -> Option<WorkerGuard> {
    use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
    use tracing_log::LogTracer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::{EnvFilter, Registry};

    if !env::var("JSON_LOG").map_or(false, |s| s.parse().unwrap_or_default()) {
        tracing_subscriber::fmt::init();
        return None;
    }

    // Redirect the logs from log library to tracing's subscribers.
    LogTracer::init().expect("Unable to setup log tracer!");

    let app_name = concat!(env!("CARGO_PKG_NAME"), "-", env!("CARGO_PKG_VERSION")).to_string();

    // Non-blocking stdout writer
    let (non_blocking_writer, guard) = tracing_appender::non_blocking(std::io::stdout());

    let bunyan_formatting_layer = BunyanFormattingLayer::new(app_name, non_blocking_writer);
    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env())
        .with(JsonStorageLayer)
        .with(bunyan_formatting_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();
    Some(guard)
}
