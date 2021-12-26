use std::net::SocketAddr;

use axum::{routing::get, Router};

use crate::web::handlers::health;
use crate::web::handlers::test_rs;

pub async fn start_server() {
    let app = Router::new()
        .route("/ping/", get(health::health))
        .route("/get-value-from-rs/", get(test_rs::get_key_from_rs));

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    tracing::warn!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
