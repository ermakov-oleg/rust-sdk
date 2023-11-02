use std::any::TypeId;
use std::borrow::{Borrow, BorrowMut};
use std::collections::HashMap;
use std::env::var;
use std::hash::Hash;
use std::sync::RwLock;

use lazy_static::lazy_static;
use serde::de::DeserializeOwned;

use crate::entities::{Setting, SettingKey};
use crate::filters::SettingsService;
use crate::providers::{
    FileProvider, MicroserviceRuntimeSettingsProvider, RuntimeSettingsState, SettingsProvider,
};
use crate::Context;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

lazy_static! {
    static ref RUNTIME_SETTINGS_BASE_URL: String = var("RUNTIME_SETTINGS_BASE_URL")
        .unwrap_or_else(|_| "http://master.runtime-settings.dev3.cian.ru".to_string());
    static ref RUNTIME_SETTINGS_FILE_PATH: String =
        var("RUNTIME_SETTINGS_FILE_PATH").unwrap_or_else(|_| "settings.json".to_string());
}

pub struct RuntimeSettings {
    settings_provider: Option<Box<dyn SettingsProvider>>,
    state: RwLock<State>,
}

struct State {
    version: String,
    settings: HashMap<String, Vec<SettingsService>>,
}

impl State {
    fn new() -> Self {
        State {
            version: "0".to_string(),
            settings: HashMap::new(),
        }
    }
}

impl RuntimeSettingsState for RwLock<State> {
    fn get_version(&self) -> String {
        let state_guard = &self.read().unwrap();
        (*state_guard.version).to_string()
    }

    fn set_version(&self, version: String) {
        let mut state_guard = self.write().unwrap();
        state_guard.version = version;
    }

    fn update_settings(&self, new_settings: Vec<Setting>, to_delete: Vec<SettingKey>) {
        let new_settings_services = new_settings.into_iter().map(SettingsService::new).collect();
        let mut state_guard = self.write().unwrap();
        let current_settings = state_guard.settings.borrow_mut();
        delete_settings(current_settings, to_delete);
        merge_settings(current_settings, new_settings_services);
    }
}

impl RuntimeSettings {
    pub fn new() -> Self {
        let mcs_settings_provider =
            MicroserviceRuntimeSettingsProvider::new(RUNTIME_SETTINGS_BASE_URL.to_string());
        Self {
            state: RwLock::new(State::new()),
            settings_provider: Some(Box::new(mcs_settings_provider)),
        }
    }

    pub fn new_with_settings_provider(provider: Box<dyn SettingsProvider>) -> Self {
        Self {
            state: RwLock::new(State::new()),
            settings_provider: Some(provider),
        }
    }

    pub async fn init(&self) {
        self.load_from_file().await
    }

    pub async fn refresh(&self) -> Result<()> {
        if let Some(settings_provider) = &self.settings_provider {
            settings_provider.update_settings(&self.state).await;
        }

        tracing::debug!("Settings refreshed");
        Ok(())
    }

    async fn load_from_file(&self) {
        let provider = FileProvider::new(RUNTIME_SETTINGS_FILE_PATH.to_string());
        provider.update_settings(&self.state).await;
    }

    pub fn get<K: ?Sized, V>(&self, key: &K, ctx: &Context) -> Option<V>
    where
        String: Borrow<K>,
        K: Hash + Eq,
        V: DeserializeOwned + 'static,
    {
        let state_guard = self.state.read().unwrap();
        let mut value = match state_guard.settings.get(key) {
            Some(vss) => vss
                .iter()
                .rev()
                .find(|f| f.is_suitable(ctx))
                .and_then(|val| val.setting.value.clone()),
            None => None,
        };

        if TypeId::of::<String>() == TypeId::of::<V>() {
            // Crutch: If a value of String type is requested, optionally wrap the value in quotes,
            // otherwise serde_json::from_str may end up with an error.
            // I couldn't skip serialization for String :(

            value = value.map(|v| match v.starts_with('\'') {
                true => v,
                false => format!("\"{}\"", v),
            })
        }
        value.and_then(|v| {
            serde_json::from_str(&v)
                .map_err(|err| {
                    tracing::error!("Error when deserialize value {}", err);
                })
                .ok()
        })
    }
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self::new()
    }
}

fn delete_settings(settings: &mut HashMap<String, Vec<SettingsService>>, deleted: Vec<SettingKey>) {
    for del in deleted {
        let SettingKey { key, priority } = del;
        settings.entry(key).and_modify(|i| {
            if let Ok(idx) = i.binary_search_by(|s| s.setting.priority.cmp(&priority)) {
                i.remove(idx);
            }
        });
    }
}

fn merge_settings(
    settings: &mut HashMap<String, Vec<SettingsService>>,
    new_settings: Vec<SettingsService>,
) {
    for new in new_settings {
        let entry = settings.entry(new.setting.key.clone()).or_default();

        match entry.binary_search_by(|item| item.setting.priority.cmp(&new.setting.priority)) {
            Ok(idx) => entry[idx] = new,
            Err(idx) => entry.insert(idx, new),
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde::Deserialize;

    use crate::entities::{RuntimeSettingsResponse, Setting};

    use super::*;

    #[derive(Deserialize, Debug, PartialEq)]
    struct SomeData {
        key: String,
    }

    fn make_ss(key: &str, priority: u32, value: Option<String>) -> SettingsService {
        SettingsService::new(Setting {
            key: key.into(),
            priority,
            value,
            runtime: "rust".into(),
            filter: HashMap::new(),
        })
    }

    #[test]
    fn test_merge_settings_empty_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> = HashMap::new();
        let new_settings = vec![make_ss("foo", 10, None), make_ss("bar", 0, None)];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([
                ("foo".to_string(), vec![make_ss("foo", 10, None)]),
                ("bar".to_string(), vec![make_ss("bar", 0, None)]),
            ])
        );
    }

    #[test]
    fn test_merge_settings_insert_settings_with_same_name() {
        let mut settings: HashMap<String, Vec<SettingsService>> = HashMap::new();
        let new_settings = vec![
            make_ss("foo", 10, None),
            make_ss("foo", 0, None),
            make_ss("foo", 30, None),
        ];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([(
                "foo".to_string(),
                vec![
                    make_ss("foo", 0, None),
                    make_ss("foo", 10, None),
                    make_ss("foo", 30, None),
                ]
            ),])
        );
    }

    #[test]
    fn test_merge_settings_extends_existed_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> =
            HashMap::from([("foo".to_string(), vec![make_ss("foo", 10, None)])]);
        let new_settings = vec![make_ss("foo", 0, None), make_ss("foo", 30, None)];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([(
                "foo".to_string(),
                vec![
                    make_ss("foo", 0, None),
                    make_ss("foo", 10, None),
                    make_ss("foo", 30, None),
                ]
            ),])
        );
    }

    #[test]
    fn test_merge_settings_swap_existed_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> =
            HashMap::from([("foo".to_string(), vec![make_ss("foo", 10, None)])]);
        let new_settings = vec![
            make_ss("foo", 0, None),
            make_ss("foo", 10, Some("new_value".to_string())),
            make_ss("foo", 30, None),
        ];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([(
                "foo".to_string(),
                vec![
                    make_ss("foo", 0, None),
                    make_ss("foo", 10, Some("new_value".to_string())),
                    make_ss("foo", 30, None),
                ]
            ),])
        );
    }

    #[test]
    fn test_delete_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> =
            HashMap::from([("foo".to_string(), vec![make_ss("foo", 10, None)])]);
        delete_settings(
            &mut settings,
            vec![
                SettingKey {
                    key: "foo".to_string(),
                    priority: 10,
                },
                SettingKey {
                    key: "bar".to_string(),
                    priority: 0,
                },
            ],
        );

        assert_eq!(settings, HashMap::from([("foo".to_string(), vec![])]));
    }

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
        data: RuntimeSettingsResponse,
    }

    #[async_trait]
    impl SettingsProvider for TestSettingsProvider {
        async fn update_settings(&self, state: &dyn RuntimeSettingsState) {
            state.update_settings(self.data.settings.clone(), self.data.deleted.clone());
            state.set_version(self.data.version.clone());
        }
    }

    #[tokio::test]
    async fn test_runtime_settings_refresh() {
        let settings_provider = TestSettingsProvider {
            data: RuntimeSettingsResponse {
                settings: vec![Setting {
                    key: "TEST_KEY".to_string(),
                    priority: 0,
                    value: Some("{\"key\": \"value\"}".to_string()),
                    runtime: "rust".into(),
                    filter: HashMap::new(),
                }],
                version: '1'.to_string(),
                deleted: vec![],
            },
        };

        let runtime_settings =
            RuntimeSettings::new_with_settings_provider(Box::new(settings_provider));
        let key = "TEST_KEY";

        // act
        let val1: Option<SomeData> = runtime_settings.get(key, &_make_ctx());

        runtime_settings.refresh().await.unwrap();

        let val2: Option<SomeData> = runtime_settings.get(key, &_make_ctx());

        // assert
        assert_eq!(val1, None);
        assert_eq!(
            val2,
            Some(SomeData {
                key: "value".to_string()
            })
        );
    }

    #[tokio::test]
    async fn test_runtime_settings_get_skip_not_suitable_settings() {
        let settings_provider = TestSettingsProvider {
            data: RuntimeSettingsResponse {
                settings: vec![
                    Setting {
                        key: "TEST_KEY".to_string(),
                        priority: 0,
                        value: Some("{\"key\": \"wrong-value\"}".to_string()),
                        runtime: "rust".into(),
                        filter: HashMap::from([("application".to_string(), "foo".to_string())]),
                    },
                    Setting {
                        key: "TEST_KEY".to_string(),
                        priority: 0,
                        value: Some("{\"key\": \"right-value\"}".to_string()),
                        runtime: "rust".into(),
                        filter: HashMap::from([(
                            "application".to_string(),
                            "test-rust".to_string(),
                        )]),
                    },
                ],
                version: '1'.to_string(),
                deleted: vec![],
            },
        };

        let runtime_settings =
            RuntimeSettings::new_with_settings_provider(Box::new(settings_provider));
        let key = "TEST_KEY";
        runtime_settings.refresh().await.unwrap();

        // act
        let val: Option<SomeData> = runtime_settings.get(key, &_make_ctx());

        // assert
        assert_eq!(
            val,
            Some(SomeData {
                key: "right-value".to_string()
            })
        );
    }
}
