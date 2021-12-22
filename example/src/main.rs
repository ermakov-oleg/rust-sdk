use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use serde::Deserialize;
use tokio::{
    task,
    time::{sleep, Duration},
};

use runtime_settings::{Context, RuntimeSettings};

#[derive(Debug, Deserialize)]
struct PGConnectionString {
    user: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let rs = init_runtime_settings().await;

    let ctx = Context {
        application: "test-rust".into(),
        server: "test-server".into(),
        environment: HashMap::from([("TEST".to_string(), "ermakov".to_string())]),
        host: None,
        url: None,
        url_path: None,
        email: None,
        ip: None,
        context: Default::default(),
    };

    let _key = "postgres_connection/some_db";
    let key = "AB_EXPERIMENTS_TIMEOUT";

    loop {
        let val: Option<u32> = rs.get(key, &ctx);
        println!("Settings {}:{:#?}", key, val);
        sleep(Duration::from_secs(5)).await;
    }
}

async fn init_runtime_settings() -> Arc<RuntimeSettings> {
    let runtime_settings = RuntimeSettings::new();
    runtime_settings.init().await;
    runtime_settings.refresh().await.unwrap();

    let settings = Arc::new(runtime_settings);
    let settings_p = Arc::clone(&settings);

    task::spawn(async move {
        loop {
            sleep(Duration::from_secs(10)).await;
            println!("Update RS started");
            let _ = settings_p
                .refresh()
                .await
                .or_else::<Box<dyn Error>, _>(|e| {
                    println!("Error when update RS {}", e);
                    Ok(())
                });
        }
    });

    settings
}
