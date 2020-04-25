use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::iter::Iterator;

use serde::de::DeserializeOwned;

use crate::context::Context;
use crate::filters::SettingsService;
use crate::providers::RuntimeSettingsProvider;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct RuntimeSettings {
    settings: HashMap<String, Vec<SettingsService>>,
    settings_provider: Box<dyn RuntimeSettingsProvider>,
}

impl RuntimeSettings {
    pub fn new<T: RuntimeSettingsProvider + 'static>(settings_provider: T) -> Self {
        Self {
            settings: HashMap::new(),
            settings_provider: Box::new(settings_provider),
        }
    }

    pub async fn refresh(&mut self) -> Result<()> {
        let new_settings = match self.settings_provider.get_settings().await {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Error: Could not update settings {}", err);
                return Ok(());
            }
        };
        self.settings = new_settings;
        println!("Settings refreshed");
        Ok(())
    }

    pub async fn refresh_with_settings_provider(
        &mut self,
        settings_provider: &mut dyn RuntimeSettingsProvider,
    ) -> Result<()> {
        let new_settings = match settings_provider.get_settings().await {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Error: Could not update settings {}", err);
                return Ok(());
            }
        };
        self.settings = new_settings;
        println!("Settings refreshed");
        Ok(())
    }

    pub fn get<K: ?Sized, V>(&self, key: &K, ctx: &Context) -> Option<V>
    where
        String: Borrow<K>,
        K: Hash + Eq,
        V: DeserializeOwned,
    {
        let value = match self.settings.get(key) {
            Some(vss) => vss
                .iter()
                .find(|f| f.is_suitable(ctx))
                .and_then(|val| val.setting.value.clone()),
            None => None,
        };

        value.and_then(|v| {
            serde_json::from_str(&v)
                .map_err(|err| {
                    eprintln!("Error when deserialize value {}", err);
                })
                .ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Setting;
    use async_trait::async_trait;
    use serde::Deserialize;

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

    fn _make_ctx() -> Context {
        Context {
            application: "test-rust".to_string(),
            server: "test-server".to_string(),
            environment: Default::default(),
            host: None,
            url: None,
            url_path: None,
            email: None,
            ip: None,
            context: Default::default(),
        }
    }

    struct TestSettingsProvider {
        settings: HashMap<String, Vec<SettingsService>>,
    }

    #[async_trait]
    impl RuntimeSettingsProvider for TestSettingsProvider {
        async fn get_settings(&mut self) -> Result<HashMap<String, Vec<SettingsService>>> {
            let settings = self.settings.drain();
            Ok(settings.collect())
        }
    }

    #[tokio::test]
    async fn test_runtime_settings_refresh() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct SomeData {
            key: String,
        }

        let mut settings = HashMap::new();
        settings.insert(
            "TEST_KEY".to_string(),
            vec![SettingsService::new(Setting {
                key: "TEST_KEY".to_string(),
                priority: 0,
                runtime: "rust".to_string(),
                filters: None,
                value: Some("{\"key\": \"value\"}".into()),
            })],
        );
        let settings_provider = TestSettingsProvider { settings };
        let mut runtime_settings = RuntimeSettings::new(settings_provider);

        runtime_settings.refresh().await.unwrap();

        let key = "TEST_KEY";

        // act
        let val: Option<SomeData> = runtime_settings.get(key, &_make_ctx());

        // assert
        assert_eq!(
            val,
            Some(SomeData {
                key: "value".to_string()
            })
        );
    }
}
