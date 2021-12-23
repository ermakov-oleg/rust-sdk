use axum::extract::Query;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::consts::APPLICATION_NAME;

#[derive(Debug, Deserialize)]
pub struct HealthRequest {
    noresponse: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult<'a> {
    success: bool,
    application_name: &'a str,
    server: &'a str,
    version: &'a str,
    mcs_run_env: &'a str,
}

#[instrument]
pub async fn health<'a>(Query(params): Query<HealthRequest>) -> Json<Option<HealthResult<'a>>> {
    let result = match params.noresponse {
        Some(_) => None,
        _ => Some(HealthResult {
            application_name: APPLICATION_NAME,
            success: true,
            server: "",
            version: "",
            mcs_run_env: "",
        }),
    };
    Json(result)
}
