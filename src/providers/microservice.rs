use std::cmp::Reverse;
use std::collections::HashMap;

use bytes::buf::BufExt;
use bytes::Buf;
use hyper::Client;
use serde::Deserialize;

use async_trait::async_trait;

use crate::entities::{RuntimeSettingsResponse, Setting};
use crate::filters::SettingsService;
use core::fmt;
use std::error;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;


#[async_trait]
pub trait DiffSettings {
    async fn get_settings(&self, version: &str) -> Result<RuntimeSettingsResponse>;
}


pub struct MicroserviceRuntimeSettingsProvider {
    base_url: String,
}

impl MicroserviceRuntimeSettingsProvider {
    pub fn new(base_url: String) -> Self {
        MicroserviceRuntimeSettingsProvider { base_url }
    }
}

#[async_trait]
impl DiffSettings for MicroserviceRuntimeSettingsProvider {
    async fn get_settings(&self, version: &str) -> Result<RuntimeSettingsResponse> {
        let url = format!(
            "{}/v2/get-runtime-settings/?runtime=python&version={}",
            self.base_url, version,
        )
        .parse()?;
        println!("Get runtime settings");
        let response: RuntimeSettingsResponse = fetch_json(url).await?;

        Ok(response)
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
    let client = Client::new();

    // Fetch the url...
    let res = client.get(url).await?;

    let (parts, body) = res.into_parts();

    // asynchronously aggregate the chunks of the body
    let body = hyper::body::aggregate(body).await?;

    if !parts.status.is_success() {
        return Err(Box::new(HttpError {
            error: String::from_utf8_lossy(body.bytes()).into_owned(),
        }));
    }

    // try to parse as json with serde_json
    let result = serde_json::from_reader(body.reader())?;

    // serde_json::from_str()
    Ok(result)
}
