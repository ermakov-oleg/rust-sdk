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
    settings: HashMap<String, Vec<Box<SettingsService>>>,
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

    pub fn get<K: ?Sized, V>(&self, key: &K, ctx: &Context) -> Option<V>
    where
        String: Borrow<K>,
        K: Hash + Eq,
        V: DeserializeOwned,
    {
        let value = match self.settings.get(key) {
            Some(vss) => vss
                .into_iter()
                .find(|f| f.is_suitable(ctx))
                .and_then(|val| val.setting.value.clone()),
            None => None,
        };

        value.map_or(None, |v| {
            serde_json::from_str(&v)
                .map_err(|err| {
                    eprintln!("Error when deserialize value {}", err);
                })
                .ok()
        })
    }
}
