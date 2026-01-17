//! Middleware for extracting runtime-settings context from HTTP requests.

use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use std::collections::HashMap;

/// Middleware that extracts request info and sets it as task-local context.
pub async fn runtime_settings_context(request: Request<Body>, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // Extract headers into HashMap
    let headers: HashMap<String, String> = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect();

    let rs_request = runtime_settings::Request {
        method,
        path,
        headers,
    };

    // Execute the rest of the handler with task-local request context
    runtime_settings::with_task_request(rs_request, next.run(request)).await
}
