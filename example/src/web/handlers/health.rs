use axum::Json;
use serde::Serialize;

use crate::consts::APPLICATION_NAME;

#[derive(Serialize)]
pub struct HealthResult<'a> {
    success: bool,
    application_name: &'a str,
    server: &'a str,
    version: &'a str,
    mcs_run_env: &'a str,
}

pub async fn health<'a>() -> Json<HealthResult<'a>> {
    let result = HealthResult {
        application_name: APPLICATION_NAME,
        success: true,
        server: "",
        version: "",
        mcs_run_env: "",
    };
    Json(result)
}
