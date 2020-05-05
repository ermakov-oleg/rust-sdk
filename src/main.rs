#![warn(rust_2018_idioms)]

use std::collections::HashMap;

use serde::Deserialize;

use runtime_settings::{Context, MicroserviceRuntimeSettingsProvider, RuntimeSettings};
use std::sync::Arc;
use std::time::Duration;
use tokio::task;
use tokio::time::delay_for;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Settings {
    runtime_settings: Arc<RuntimeSettings>,
}

#[derive(Debug, Deserialize)]
struct PGConnectionString {
    user: String,
    password: String,
}

impl Settings {
    async fn new() -> Self {
        Self {
            runtime_settings: Self::init_runtime_settings().await,
        }
    }

    async fn init_runtime_settings() -> Arc<RuntimeSettings> {
        let settings_provider = MicroserviceRuntimeSettingsProvider::new(
            "http://master.runtime-settings.dev3.cian.ru".into(),
        );
        let runtime_settings = RuntimeSettings::new(settings_provider);

        runtime_settings.refresh().await.unwrap(); // todo: fallback

        let settings = Arc::new(runtime_settings);

        let settings_p = Arc::clone(&settings);

        task::spawn(async move {
            loop {
                delay_for(Duration::from_secs(10)).await;
                println!("Update RS started");
                settings_p.refresh().await.ok();
                // or_else(|e| {
                //     println!("Error when update RS {}", e);
                //     Ok(())
                // });
            }
        });

        settings
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = Settings::new().await;

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

    // let key = "postgres_connection/qa_tests_manager";
    let key = "CALLTRACKING_CORE_TIMEOUT";

    let val: Option<u32> = app.runtime_settings.get(key, &ctx);
    // let val: Option<String> = runtime_settings.get(key, &ctx);

    println!("Settings {}:{:#?}", key, val);

    delay_for(Duration::from_secs(60)).await;
    Ok(())
}
