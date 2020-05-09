use std::cmp::Reverse;
use std::collections::HashMap;

use bytes::buf::BufExt;
use bytes::Buf;
use hyper::Client;
use serde::Deserialize;

use async_trait::async_trait;

use crate::entities::Setting;
use crate::filters::SettingsService;
use crate::RuntimeSettingsProvider;
use core::fmt;
use std::error;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct MicroserviceRuntimeSettingsProvider {
    base_url: String,
}

impl MicroserviceRuntimeSettingsProvider {
    pub fn new(base_url: String) -> Self {
        MicroserviceRuntimeSettingsProvider { base_url }
    }
}

#[derive(Deserialize, Debug)]
struct SettingKey {
    key: String,
    priority: u32,
}

#[derive(Deserialize, Debug)]
struct RuntimeSettingsResponse {
    settings: Vec<Setting>,
    deleted: Vec<SettingKey>,
    version: String,
}

#[async_trait]
impl RuntimeSettingsProvider for MicroserviceRuntimeSettingsProvider {
    async fn get_settings(&self) -> Result<HashMap<String, Vec<SettingsService>>> {
        let url = format!(
            "{}/v2/get-cian-settings/?runtime=python&version=0",
            self.base_url
        )
        .parse()?;
        println!("Get runtime settings");
        let rs_response: RuntimeSettingsResponse = fetch_json(url).await?;

        let settings = prepare_settings(rs_response.settings);

        Ok(settings)
    }
}

fn prepare_settings(settings: Vec<Setting>) -> HashMap<String, Vec<SettingsService>> {
    let mut settings_dict = HashMap::new();
    for s in settings {
        let key = s.key.clone();
        let ss = SettingsService::new(s);

        settings_dict.entry(key).or_insert_with(Vec::new).push(ss);
    }
    settings_dict
        .values_mut()
        .for_each(|data| data.sort_by_key(|ss| Reverse(ss.setting.priority)));
    settings_dict
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

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_settings_expected_settings_sort_order() {
        // arrange
        let raw_settings = vec![
            Setting {
                key: "foo".to_string(),
                priority: 100,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            },
            Setting {
                key: "foo".to_string(),
                priority: 0,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            },
            Setting {
                key: "foo".to_string(),
                priority: 110,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            },
        ];

        // act
        let settings = prepare_settings(raw_settings);

        // assert
        assert_eq!(
            settings["foo"]
                .iter()
                .map(|s| s.setting.priority)
                .collect::<Vec<u32>>(),
            [110, 100, 0]
        );
    }
}
