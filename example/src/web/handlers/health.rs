use axum::extract::Query;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::consts::APPLICATION_NAME;

#[derive(Debug, Deserialize)]
pub struct HealthRequest {
    noresponse: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult {
    success: bool,
    application_name: &'static str,
}

pub async fn health(Query(params): Query<HealthRequest>) -> Json<Option<HealthResult>> {
    let result = match params.noresponse {
        Some(_) => None,
        _ => Some(HealthResult {
            application_name: APPLICATION_NAME,
            success: true,
        }),
    };
    Json(result)
}
