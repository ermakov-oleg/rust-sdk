use std::net::SocketAddr;

use axum::{routing::get, Router};

use crate::web::handlers::health;

pub async fn start_server() {
    let app = Router::new().route("/ping/", get(health));

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    tracing::warn!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
