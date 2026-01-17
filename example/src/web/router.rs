use axum::{middleware, routing::get, Router};
use clap::Args;
use std::sync::Arc;

use crate::middleware::runtime_settings_context;
use crate::web::handlers::health;
use crate::web::handlers::test_rs;
use runtime_settings::RuntimeSettings;

#[derive(Debug, Args)]
pub struct Serve {
    /// Run on host
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// Listen port
    #[arg(short, long, default_value = "8000")]
    pub port: u16,
}

pub async fn start_server(params: Serve, settings: Arc<RuntimeSettings>) {
    let app = Router::new()
        .route("/ping/", get(health::health))
        .route("/get-value-from-rs/", get(test_rs::get_key_from_rs))
        .layer(middleware::from_fn(runtime_settings_context))
        .with_state(settings);

    let addr = format!("{}:{}", params.host, params.port);
    tracing::warn!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind listener");
    axum::serve(listener, app).await.unwrap();
}
