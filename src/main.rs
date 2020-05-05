#![warn(rust_2018_idioms)]

use std::collections::HashMap;

use serde::Deserialize;

use cian_settings::{Context, Settings};
use std::time::Duration;
use tokio::time::delay_for;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Deserialize)]
struct PGConnectionString {
    user: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let settings = Settings::new("http://master.cian-settings.dev3.cian.ru".into()).await;

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
    // let key = "CALLTRACKING_CORE_TIMEOUT";

    // let val: Option<u32> = settings.get(key, &ctx);
    let val: Option<PGConnectionString> = settings.get(key, &ctx);

    println!("Settings {}:{:#?}", key, val);

    delay_for(Duration::from_secs(60)).await;
    Ok(())
}
