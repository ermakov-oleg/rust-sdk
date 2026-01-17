use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use runtime_settings::RuntimeSettings;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Result {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
pub struct Request {
    key: Option<String>,
}

pub async fn get_key_from_rs(
    State(settings): State<Arc<RuntimeSettings>>,
    Query(params): Query<Request>,
) -> Json<Result> {
    let key = params.key.unwrap_or_else(|| "SOME_KEY".to_string());

    // Context is automatically available from middleware via task-local storage
    let value: Option<Arc<String>> = settings.get(&key);
    let ser_value = serde_json::to_string(&value.as_deref()).unwrap();

    Json(Result {
        key,
        value: ser_value,
    })
}
