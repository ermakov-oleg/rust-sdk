use std::cmp::Reverse;
use std::collections::HashMap;

use bytes::buf::BufExt;
use hyper::Client;
use serde::Deserialize;

use async_trait::async_trait;

use crate::entities::Setting;
use crate::filters::SettingsService;
use crate::RuntimeSettingsProvider;

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
    settings: Vec<Box<Setting>>,
    deleted: Vec<Box<SettingKey>>,
    version: String,
}

#[async_trait]
impl RuntimeSettingsProvider for MicroserviceRuntimeSettingsProvider {
    async fn get_settings(&self) -> Result<HashMap<String, Vec<Box<SettingsService>>>> {
        let url = format!(
            "{}/v2/get-runtime-settings/?runtime=python&version=0",
            self.base_url
        )
        .parse()
        .unwrap();
        println!("Get runtime settings");
        let rs_response: RuntimeSettingsResponse = fetch_json(url).await?;

        println!("Settings: {:#?}", rs_response);

        let settings = prepare_settings(rs_response.settings);

        Ok(settings)
    }
}

fn prepare_settings(settings: Vec<Box<Setting>>) -> HashMap<String, Vec<Box<SettingsService>>> {
    let mut settings_dict = HashMap::new();
    for s in settings {
        let key = s.key.clone();
        let ss = SettingsService::new(s);

        settings_dict
            .entry(key.into())
            .or_insert_with(Vec::new)
            .push(Box::new(ss));
    }
    settings_dict
        .values_mut()
        .for_each(|data| data.sort_by_key(|ss| Reverse(ss.setting.priority)));
    settings_dict
}

async fn fetch_json<T>(url: hyper::Uri) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let client = Client::new();

    // Fetch the url...
    let res = client.get(url).await?;

    // asynchronously aggregate the chunks of the body
    let body = hyper::body::aggregate(res).await?;

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
            Box::new(Setting {
                key: "foo".to_string(),
                priority: 100,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            }),
            Box::new(Setting {
                key: "foo".to_string(),
                priority: 0,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            }),
            Box::new(Setting {
                key: "foo".to_string(),
                priority: 110,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            }),
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
