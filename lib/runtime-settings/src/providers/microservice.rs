use core::fmt;
use std::error;

use async_trait::async_trait;
use hyper::body::{to_bytes, Buf};
use hyper::Client;
use hyper_tls::HttpsConnector;

use crate::entities::RuntimeSettingsResponse;
use crate::providers::{Result, RuntimeSettingsState, SettingsProvider};

pub struct MicroserviceRuntimeSettingsProvider {
    base_url: String,
}

impl MicroserviceRuntimeSettingsProvider {
    pub fn new(base_url: String) -> Self {
        MicroserviceRuntimeSettingsProvider { base_url }
    }

    async fn get_settings(&self, version: &str) -> Result<RuntimeSettingsResponse> {
        let url = format!(
            "{}/v2/get-runtime-settings/?runtime=rust&version={}",
            self.base_url, version,
        )
        .parse()?;
        tracing::debug!("Get runtime settings {:?}", url);
        let response: RuntimeSettingsResponse = fetch_json(url).await?;

        Ok(response)
    }
}

#[async_trait]
impl SettingsProvider for MicroserviceRuntimeSettingsProvider {
    async fn update_settings(&self, state: &dyn RuntimeSettingsState) {
        tracing::debug!("Refresh settings ...");
        let version = state.get_version();
        let diff = match self.get_settings(&version).await {
            Ok(r) => r,
            Err(err) => {
                tracing::error!("Error: Could not update settings {}", err);
                return;
            }
        };
        tracing::trace!("New Settings {:?}", &diff);
        state.update_settings(diff.settings, diff.deleted);
        state.set_version(diff.version);
    }
}

#[derive(Debug, Clone)]
struct HttpError {
    error: String,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid http request {}", self.error)
    }
}

impl error::Error for HttpError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

async fn fetch_json<T>(url: hyper::Uri) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    // Create client
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    // Fetch the url...
    let res = client.get(url).await?;

    let (parts, body) = res.into_parts();

    // asynchronously aggregate the chunks of the body
    let body = to_bytes(body).await?;

    if !parts.status.is_success() {
        return Err(Box::new(HttpError {
            error: String::from_utf8_lossy(&*body).into_owned(),
        }));
    }

    // try to parse as json with serde_json
    let result = serde_json::from_reader(body.reader())?;

    Ok(result)
}
