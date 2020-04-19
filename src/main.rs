#![warn(rust_2018_idioms)]

use std::collections::HashMap;

use serde::Deserialize;

use runtime_settings::{Context, MicroserviceRuntimeSettingsProvider, RuntimeSettings};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Deserialize)]
struct PGConnectionString {
    user: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let settings_provider = MicroserviceRuntimeSettingsProvider::new(
        "http://master.runtime-settings.dev3.cian.ru".into(),
    );
    let mut runtime_settings = RuntimeSettings::new(settings_provider);

    runtime_settings.refresh().await?;

    let ctx = Context {
        application: "test-rust".into(),
        server: "test-server".into(),
        environment: HashMap::new(),
        host: None,
        url: None,
        url_path: None,
        email: None,
        ip: None,
        context: Default::default(),
    };

    let key = "postgres_connection/qa_tests_manager";
    // let key = "isNewPublishTerms.Enabled";

    let val: Option<PGConnectionString> = runtime_settings.get(key, &ctx);
    // let val: Option<String> = runtime_settings.get(key, &ctx);

    println!("Settings {}:{:#?}", key, val);

    Ok(())
}
