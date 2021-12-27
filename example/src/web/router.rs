use axum::{routing::get, Router};
use structopt::StructOpt;

use crate::web::handlers::health;
use crate::web::handlers::test_rs;

#[derive(Debug, StructOpt)]
pub struct Serve {
    /// Run on host
    #[structopt(short, long, default_value = "127.0.0.1")]
    pub host: String,

    /// Listen port
    #[structopt(short, long, default_value = "8000")]
    pub port: u16,
}

pub async fn start_server(params: Serve) {
    let app = Router::new()
        .route("/ping/", get(health::health))
        .route("/get-value-from-rs/", get(test_rs::get_key_from_rs));

    let addr = format!("{}:{}", params.host, params.port);
    tracing::warn!("listening on {}", addr);
    axum::Server::bind(&addr.parse().expect("Invalid listen addr"))
        .serve(app.into_make_service())
        .await
        .unwrap();
}
