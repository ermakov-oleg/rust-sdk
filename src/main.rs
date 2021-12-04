#![warn(rust_2018_idioms)]


use std::collections::HashMap;

use serde::Deserialize;

use cian_settings::{Context, RuntimeSettings, test};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

//
#[derive(Debug, Deserialize)]
struct PGConnectionString {
    user: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    test();

    let mut rs = RuntimeSettings::new();
    rs.init().await;
    rs.refresh().await.unwrap();


    let ctx = Context {
        application: "test-rust".into(),
        server: "test-server".into(),
        environment: HashMap::from([
            ("TEST".to_string(), "ermakov".to_string()),
        ]),
        host: None,
        url: None,
        url_path: None,
        email: None,
        ip: None,
        context: Default::default(),
    };

    let key = "postgres_connection/some_db";
    let key = "SOME_KEY";

    let val: Option<u32> = rs.get(key, &ctx);

    println!("Settings {}:{:#?}", key, val);

    Ok(())
}
