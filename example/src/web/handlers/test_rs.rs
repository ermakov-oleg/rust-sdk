use axum::extract::Query;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::to_string;
use tracing::instrument;

use runtime_settings;

use crate::settings::get_context;

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

#[instrument]
pub async fn get_key_from_rs(Query(params): Query<Request>) -> Json<Result> {
    let key = params.key.unwrap_or_else(|| "SOME_KEY".to_string());
    let value: Option<String> = runtime_settings::settings.get(&key, &get_context());
    let ser_value = to_string(&value).unwrap();
    let result = Result {
        key,
        value: ser_value,
    };
    Json(result)
}
